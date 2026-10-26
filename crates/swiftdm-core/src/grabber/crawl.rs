//! Exploring a site: pages are read breadth first, a few at a time with a pause between
//! rounds, within the project's link levels, folder and address rules, and robots.txt.
//! Files the project wants are checked with a HEAD request (size, type) and listed.
use super::html;
use super::{url_ext, GrabberEvent, Item, ItemKind, ItemStatus, Project, SaveMode, Stats, Template, PAGE_EXTS};
use crate::cancel::CancelToken;
use crate::manager::{Manager, StartMode};
use crate::models::JobSpec;
use crate::settings::Settings;
use crate::store::Store;
use futures_util::future::join_all;
use futures_util::StreamExt;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast;

/// Pages larger than this are not read for links.
const MAX_PAGE_BYTES: usize = 5 * 1024 * 1024;

pub struct Grabber {
    store: Arc<Store>,
    mgr: Manager,
    tx: broadcast::Sender<GrabberEvent>,
    running: Mutex<HashMap<String, CancelToken>>,
}

struct Pending {
    url: url::Url,
    /// Link levels followed on the start site, and on other sites, to reach this page.
    here: u32,
    other: u32,
    parent: Option<String>,
    text: Option<String>,
}

impl Grabber {
    pub fn new(store: Arc<Store>, mgr: Manager) -> Arc<Self> {
        let (tx, _) = broadcast::channel(4096);
        Arc::new(Grabber { store, mgr, tx, running: Mutex::default() })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<GrabberEvent> {
        self.tx.subscribe()
    }

    pub fn projects(&self) -> Vec<Project> {
        self.store.grabber_projects().unwrap_or_default()
    }

    pub fn project(&self, id: &str) -> Option<Project> {
        self.projects().into_iter().find(|p| p.id == id)
    }

    /// Save a new or changed project; returns it with its id and times set.
    pub fn save_project(&self, p: Project) -> Result<Project, String> {
        let mut p = p.normalized();
        let u = url::Url::parse(&p.start_url).map_err(|_| "Type the full address of the start page, e.g. https://example.com/docs/".to_string())?;
        if !matches!(u.scheme(), "http" | "https") {
            return Err("The start page must be an http or https address.".into());
        }
        let now = crate::now();
        if p.id.is_empty() {
            p.id = uuid::Uuid::new_v4().simple().to_string();
            p.created_at = now;
        }
        p.updated_at = now;
        self.store.save_grabber_project(&p).map_err(|e| e.to_string())?;
        Ok(p)
    }

    pub fn delete_project(&self, id: &str) -> Result<(), String> {
        self.stop(id);
        self.store.delete_grabber_project(id).map_err(|e| e.to_string())
    }

    pub fn items(&self, id: &str) -> Vec<Item> {
        self.store.grabber_items(id).unwrap_or_default()
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.running.lock().unwrap().contains_key(id)
    }

    pub fn stop(&self, id: &str) {
        if let Some(c) = self.running.lock().unwrap().get(id) {
            c.set(crate::cancel::StopReason::Pause);
        }
    }

    /// Start exploring a project in the background. What an earlier run found is kept, except
    /// files that were not downloaded (they are listed again if still linked).
    pub fn explore(self: &Arc<Self>, id: &str) -> Result<(), String> {
        let p = self.project(id).ok_or("This project no longer exists.")?;
        let cancel = CancelToken::new();
        {
            let mut r = self.running.lock().unwrap();
            if r.contains_key(id) {
                return Ok(());
            }
            r.insert(id.to_string(), cancel.clone());
        }
        let me = self.clone();
        tokio::spawn(async move {
            let reason = me.run(&p, &cancel).await;
            me.running.lock().unwrap().remove(&p.id);
            let _ = me.tx.send(GrabberEvent::Done { project_id: p.id.clone(), reason });
        });
        Ok(())
    }

    pub fn set_checked(&self, id: &str, item_ids: &[i64], checked: bool) {
        for mut it in self.items(id).into_iter().filter(|i| item_ids.contains(&i.id)) {
            it.checked = checked;
            if self.store.save_grabber_item(&it).is_ok() {
                let _ = self.tx.send(GrabberEvent::Item { project_id: id.into(), item: it });
            }
        }
    }

    /// Send items to the download list; returns the new downloads' ids.
    pub fn download(&self, id: &str, item_ids: &[i64]) -> Result<Vec<String>, String> {
        let p = self.project(id).ok_or("This project no longer exists.")?;
        let s = self.mgr.settings().get();
        let mut out = vec![];
        for mut it in self.items(id).into_iter().filter(|i| item_ids.contains(&i.id) && i.job_id.is_none()) {
            let spec = spec_for(&p, &s, &it)?;
            let job = self.mgr.submit(spec, StartMode::Now)?;
            it.status = ItemStatus::Queued;
            it.job_id = Some(job.clone());
            self.store.save_grabber_item(&it).map_err(|e| e.to_string())?;
            let _ = self.tx.send(GrabberEvent::Item { project_id: id.into(), item: it });
            out.push(job);
        }
        Ok(out)
    }

    fn emit_item(&self, it: &mut Item) {
        if let Ok(id) = self.store.save_grabber_item(it) {
            it.id = id;
            let _ = self.tx.send(GrabberEvent::Item { project_id: it.project_id.clone(), item: it.clone() });
        }
    }

    async fn run(&self, p: &Project, cancel: &CancelToken) -> String {
        let settings = self.mgr.settings().get();
        let client = match client(&settings) {
            Ok(c) => c,
            Err(e) => return e,
        };
        let Ok(start) = url::Url::parse(&p.start_url) else { return "The start page address is not valid.".into() };
        let site = Site::new(&start, p.whole_domain);
        let start_dir = {
            let path = start.path();
            path[..path.rfind('/').map_or(0, |i| i + 1)].to_string()
        };
        // Downloaded or chosen files stay; the rest is found again.
        let mut seen: HashSet<String> = HashSet::new();
        let mut dupes: HashSet<(String, Option<u64>)> = HashSet::new();
        for it in self.items(&p.id) {
            if it.job_id.is_some() {
                seen.insert(it.url.clone());
                if let Ok(u) = url::Url::parse(&it.url) {
                    dupes.insert((file_name(&u), it.size));
                }
            }
        }
        let mut robots = Robots::default();
        let mut stats = Stats::default();
        let mut queue = VecDeque::from([Pending { url: start.clone(), here: 0, other: 0, parent: None, text: None }]);
        seen.insert(start.to_string());
        while !queue.is_empty() {
            if cancel.is_set() {
                return "stopped".into();
            }
            if stats.pages_explored >= p.max_pages {
                return format!("stopped after {} pages (the project's limit)", p.max_pages);
            }
            let take = (p.parallel as usize).min(queue.len()).min((p.max_pages - stats.pages_explored) as usize);
            let batch: Vec<Pending> = queue.drain(..take).collect();
            let mut allowed = vec![];
            for pg in batch {
                if p.respect_robots && !robots.allows(&client, &pg.url).await {
                    let mut it = page_item(p, &pg, ItemStatus::Skipped);
                    it.note = Some("not allowed by the site's robots.txt".into());
                    self.emit_item(&mut it);
                    continue;
                }
                allowed.push(pg);
            }
            let pages = join_all(allowed.iter().map(|pg| fetch(&client, &pg.url, cancel))).await;
            let mut files: Vec<(url::Url, Pending)> = vec![];
            for (pg, got) in allowed.into_iter().zip(pages) {
                stats.pages_explored += 1;
                match got {
                    Err(e) => {
                        stats.errors += 1;
                        let mut it = page_item(p, &pg, ItemStatus::Failed);
                        it.note = Some(e);
                        self.emit_item(&mut it);
                    }
                    // Not a web page after all (a download behind a page-like address).
                    Ok(Fetched::File { final_url, content_type, size }) => {
                        if p.wants_file(&url_ext(&final_url)) || p.template == Template::Files || p.template == Template::Site {
                            self.add_file(p, &pg, final_url, content_type, size, &mut dupes, &mut stats).await;
                        }
                    }
                    Ok(Fetched::Page { final_url, body }) => {
                        let mut it = page_item(p, &pg, ItemStatus::Explored);
                        // A copy of the site downloads the pages too.
                        it.checked = p.template == Template::Site;
                        self.emit_item(&mut it);
                        for l in html::links(&body, &final_url) {
                            let key = l.url.to_string();
                            if !seen.insert(key) {
                                continue;
                            }
                            let same = site.contains(&l.url);
                            let ext = url_ext(&l.url);
                            let next = Pending {
                                url: l.url.clone(),
                                here: pg.here + same as u32,
                                other: pg.other + !same as u32,
                                parent: Some(final_url.to_string()),
                                text: l.text.clone(),
                            };
                            if PAGE_EXTS.contains(&ext.as_str()) {
                                if explorable(p, &site, &start_dir, &next) {
                                    queue.push_back(next);
                                }
                            } else if p.wants_file(&ext) && (same || !p.files_this_site_only) {
                                files.push((l.url, next));
                            }
                        }
                    }
                }
            }
            if p.respect_robots {
                let mut kept = vec![];
                for f in files {
                    if robots.allows(&client, &f.0).await {
                        kept.push(f);
                    }
                }
                files = kept;
            }
            // Sizes and types of the files found in this round.
            let heads = join_all(files.iter().map(|(u, _)| head(&client, u, cancel))).await;
            for ((u, pg), h) in files.into_iter().zip(heads) {
                let (final_url, ct, size) = h.unwrap_or((u, None, None));
                self.add_file(p, &pg, final_url, ct, size, &mut dupes, &mut stats).await;
            }
            stats.pages_waiting = queue.len() as u32;
            let _ = self.tx.send(GrabberEvent::Stats { project_id: p.id.clone(), stats });
            if !queue.is_empty() && p.delay_ms > 0 {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(p.delay_ms as u64)) => {}
                    _ = cancel.cancelled() => return "stopped".into(),
                }
            }
        }
        "finished".into()
    }

    #[allow(clippy::too_many_arguments)]
    async fn add_file(
        &self,
        p: &Project,
        pg: &Pending,
        url: url::Url,
        content_type: Option<String>,
        size: Option<u64>,
        dupes: &mut HashSet<(String, Option<u64>)>,
        stats: &mut Stats,
    ) {
        if let Some(sz) = size {
            if (p.min_size > 0 && sz < p.min_size) || (p.max_size > 0 && sz > p.max_size) {
                return;
            }
        }
        if p.hide_duplicates && !dupes.insert((file_name(&url), size)) {
            return;
        }
        let mut it = Item {
            id: 0,
            project_id: p.id.clone(),
            url: url.to_string(),
            parent_url: pg.parent.clone(),
            depth: pg.here + pg.other,
            kind: ItemKind::File,
            content_type,
            size,
            link_text: pg.text.clone(),
            checked: true,
            status: ItemStatus::Found,
            note: None,
            job_id: None,
        };
        self.emit_item(&mut it);
        stats.files_found += 1;
        if p.start_at_once {
            let _ = self.download(&p.id, &[it.id]);
        }
    }
}

fn page_item(p: &Project, pg: &Pending, status: ItemStatus) -> Item {
    Item {
        id: 0,
        project_id: p.id.clone(),
        url: pg.url.to_string(),
        parent_url: pg.parent.clone(),
        depth: pg.here + pg.other,
        kind: ItemKind::Page,
        content_type: None,
        size: None,
        link_text: pg.text.clone(),
        checked: false,
        status,
        note: None,
        job_id: None,
    }
}

/// Whether a linked page is explored under the project's rules.
fn explorable(p: &Project, site: &Site, start_dir: &str, pg: &Pending) -> bool {
    let same = site.contains(&pg.url);
    if same {
        if pg.here > p.depth || (p.no_parent && !pg.url.path().starts_with(start_dir)) {
            return false;
        }
    } else if pg.other > p.other_sites_depth {
        return false;
    }
    let u = pg.url.as_str();
    if p.exclude_paths.iter().any(|x| crate::wildcard::matches(x, u)) {
        return false;
    }
    p.include_paths.is_empty() || p.include_paths.iter().any(|x| crate::wildcard::matches(x, u))
}

/// The start page's site: its host and port, or (whole domain) any host under its domain.
struct Site {
    host: String,
    port: Option<u16>,
    domain: Option<String>,
}

impl Site {
    fn new(start: &url::Url, whole_domain: bool) -> Self {
        let host = start.host_str().unwrap_or("").to_ascii_lowercase();
        let domain = whole_domain.then(|| host.strip_prefix("www.").unwrap_or(&host).to_string());
        Site { host, port: start.port_or_known_default(), domain }
    }
    fn contains(&self, u: &url::Url) -> bool {
        let h = u.host_str().unwrap_or("").to_ascii_lowercase();
        match &self.domain {
            Some(d) => h == *d || h.ends_with(&format!(".{d}")),
            None => h == self.host && u.port_or_known_default() == self.port,
        }
    }
}

enum Fetched {
    Page { final_url: url::Url, body: String },
    File { final_url: url::Url, content_type: Option<String>, size: Option<u64> },
}

fn client(s: &Settings) -> Result<reqwest::Client, String> {
    let resolver = crate::proxy::Resolver::new(s);
    let b = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(5))
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(60))
        .user_agent(s.user_agent.clone());
    let b = if resolver.is_direct() { b.no_proxy() } else { b.proxy(reqwest::Proxy::custom(move |u| resolver.proxy_for_url(u))) };
    b.build().map_err(|e| format!("Could not start the HTTP client: {e}"))
}

fn is_html(ct: Option<&str>) -> bool {
    ct.map(|c| c.to_ascii_lowercase()).is_some_and(|c| c.starts_with("text/html") || c.starts_with("application/xhtml"))
}

async fn fetch(client: &reqwest::Client, url: &url::Url, cancel: &CancelToken) -> Result<Fetched, String> {
    let r = tokio::select! {
        r = client.get(url.clone()).send() => r.map_err(|e| short_err(&e))?,
        _ = cancel.cancelled() => return Err("stopped".into()),
    };
    if !r.status().is_success() {
        return Err(format!("the server answered {}", r.status().as_u16()));
    }
    let final_url = r.url().clone();
    let ct = r.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(str::to_string);
    if !is_html(ct.as_deref()) {
        return Ok(Fetched::File { final_url, content_type: ct, size: r.content_length() });
    }
    let mut body = Vec::new();
    let mut stream = r.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| short_err(&e))?;
        if body.len() + chunk.len() > MAX_PAGE_BYTES {
            break;
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Fetched::Page { final_url, body: String::from_utf8_lossy(&body).into_owned() })
}

async fn head(client: &reqwest::Client, url: &url::Url, cancel: &CancelToken) -> Option<(url::Url, Option<String>, Option<u64>)> {
    let r = tokio::select! {
        r = client.head(url.clone()).send() => r.ok()?,
        _ = cancel.cancelled() => return None,
    };
    if !r.status().is_success() {
        return None;
    }
    let ct = r.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(str::to_string);
    let len = r.headers().get(reqwest::header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok());
    Some((r.url().clone(), ct, len))
}

fn short_err(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "the server did not answer in time".into()
    } else if e.is_connect() {
        "could not connect".into()
    } else {
        e.to_string()
    }
}

/// robots.txt rules per site, read once: the "*" group's Disallow lines (and Allow lines,
/// which win when they are longer, as search engines do).
#[derive(Default)]
struct Robots {
    sites: HashMap<String, Vec<(bool, String)>>,
}

impl Robots {
    async fn allows(&mut self, client: &reqwest::Client, u: &url::Url) -> bool {
        let key = format!("{}://{}", u.scheme(), u.host_str().unwrap_or("")) + &u.port().map(|p| format!(":{p}")).unwrap_or_default();
        if !self.sites.contains_key(&key) {
            let text = match client.get(format!("{key}/robots.txt")).send().await {
                Ok(r) if r.status().is_success() => r.text().await.unwrap_or_default(),
                _ => String::new(),
            };
            self.sites.insert(key.clone(), parse_robots(&text));
        }
        robots_allow(&self.sites[&key], u.path())
    }
}

pub fn parse_robots(text: &str) -> Vec<(bool, String)> {
    let mut rules = vec![];
    let mut in_star = false;
    let mut last_was_agent = false;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((k, v)) = line.split_once(':') else { continue };
        let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
        match k.as_str() {
            "user-agent" => {
                // Consecutive User-agent lines form one group.
                if !last_was_agent {
                    in_star = false;
                }
                in_star |= v == "*" || v.to_ascii_lowercase().contains("swiftharrydm");
                last_was_agent = true;
            }
            "disallow" | "allow" => {
                last_was_agent = false;
                if in_star && !v.is_empty() {
                    rules.push((k == "allow", v.to_string()));
                }
            }
            _ => last_was_agent = false,
        }
    }
    rules
}

pub fn robots_allow(rules: &[(bool, String)], path: &str) -> bool {
    let hit = |pat: &str| {
        let anchored = pat.ends_with('$');
        let pat = pat.trim_end_matches('$');
        if pat.contains('*') {
            crate::wildcard::matches(&format!("{pat}{}", if anchored { "" } else { "*" }), path)
        } else if anchored {
            path == pat
        } else {
            path.starts_with(pat)
        }
    };
    let best = rules.iter().filter(|(_, p)| hit(p)).max_by_key(|(allow, p)| (p.len(), *allow));
    best.is_none_or(|(allow, _)| *allow)
}

fn file_name(u: &url::Url) -> String {
    let last = u.path_segments().and_then(|mut s| s.next_back()).unwrap_or("");
    let name = percent_encoding::percent_decode_str(last).decode_utf8_lossy().into_owned();
    crate::fs::names::sanitize_component(if name.is_empty() { "index.html" } else { &name })
}

/// The download for an item: its folder (category or the project's folder, plus the site's
/// folders when asked) and its file name.
fn spec_for(p: &Project, s: &Settings, it: &Item) -> Result<JobSpec, String> {
    let u = url::Url::parse(&it.url).map_err(|_| "A found address is not valid.".to_string())?;
    let mut name = file_name(&u);
    if it.kind == ItemKind::Page && url_ext(&u).is_empty() {
        name = format!("{}.html", name.trim_end_matches(".html"));
    }
    let mut dir: PathBuf = match p.save_mode {
        SaveMode::Categories => s.folder_for(&name, Some(&it.url)).0,
        SaveMode::Folder if p.folder.trim().is_empty() => PathBuf::from(&s.download_dir),
        SaveMode::Folder => PathBuf::from(p.folder.trim()),
    };
    if p.keep_subfolders {
        dir.push(crate::fs::names::sanitize_component(u.host_str().unwrap_or("site")));
        let segs: Vec<&str> = u.path_segments().map(|s| s.collect()).unwrap_or_default();
        for seg in segs.iter().take(segs.len().saturating_sub(1)).filter(|s| !s.is_empty()) {
            let seg = percent_encoding::percent_decode_str(seg).decode_utf8_lossy().into_owned();
            dir.push(crate::fs::names::sanitize_component(&seg));
        }
    }
    let mut spec = JobSpec::new(it.url.clone(), dir.to_string_lossy().into_owned());
    // A file whose address has no extension is named by the server (Content-Disposition).
    spec.filename = (it.kind == ItemKind::Page || !url_ext(&u).is_empty()).then_some(name);
    spec.source = "grabber".into();
    spec.referer = it.parent_url.clone();
    spec.description = it.link_text.clone();
    Ok(spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn robots_rules() {
        let r = parse_robots("User-agent: Googlebot\nDisallow: /\n\nUser-agent: *\nDisallow: /private/\nAllow: /private/open/\nDisallow: /*.cgi$\n");
        assert!(robots_allow(&r, "/docs/a.pdf"));
        assert!(!robots_allow(&r, "/private/x.pdf"));
        assert!(robots_allow(&r, "/private/open/x.pdf"), "the longer Allow wins");
        assert!(!robots_allow(&r, "/bin/run.cgi"));
        assert!(robots_allow(&r, "/bin/run.cgi.txt"));
        assert!(robots_allow(&parse_robots(""), "/anything"));
    }

    #[test]
    fn sites_and_rules() {
        let start = url::Url::parse("https://www.example.com/docs/index.html").unwrap();
        let s = Site::new(&start, false);
        let u = |x: &str| url::Url::parse(x).unwrap();
        assert!(s.contains(&u("https://www.example.com/x")));
        assert!(!s.contains(&u("https://cdn.example.com/x")));
        assert!(!s.contains(&u("https://www.example.com:8443/x")));
        let w = Site::new(&start, true);
        assert!(w.contains(&u("https://cdn.example.com/x")) && w.contains(&u("https://example.com/")));
        assert!(!w.contains(&u("https://notexample.com/")));
        let p = Project { depth: 1, no_parent: true, exclude_paths: vec!["*/print/*".into()], ..Project::default() };
        let pg = |x: &str, here, other| Pending { url: u(x), here, other, parent: None, text: None };
        assert!(explorable(&p, &s, "/docs/", &pg("https://www.example.com/docs/a.html", 1, 0)));
        assert!(!explorable(&p, &s, "/docs/", &pg("https://www.example.com/docs/b/c.html", 2, 0)), "too deep");
        assert!(!explorable(&p, &s, "/docs/", &pg("https://www.example.com/about.html", 1, 0)), "above the start folder");
        assert!(!explorable(&p, &s, "/docs/", &pg("https://www.example.com/docs/print/a.html", 1, 0)), "excluded");
        assert!(!explorable(&p, &s, "/docs/", &pg("https://other.example/a.html", 0, 1)), "other sites are off by default");
    }

    #[test]
    fn download_folders() {
        let s = Settings { download_dir: r"C:\D".into(), ..Settings::default() };
        let it = |url: &str| Item {
            id: 1,
            project_id: "p".into(),
            url: url.into(),
            parent_url: None,
            depth: 1,
            kind: ItemKind::File,
            content_type: None,
            size: None,
            link_text: None,
            checked: true,
            status: ItemStatus::Found,
            note: None,
            job_id: None,
        };
        let p = Project::default();
        let spec = spec_for(&p, &s, &it("https://a.example/pub/My%20Doc.pdf")).unwrap();
        assert_eq!((spec.dest_dir.as_str(), spec.filename.as_deref()), (r"C:\D\Documents", Some("My Doc.pdf")));
        let p2 = Project { save_mode: SaveMode::Folder, folder: r"E:\Grab".into(), keep_subfolders: true, ..Project::default() };
        let spec = spec_for(&p2, &s, &it("https://a.example/pub/v1/x.zip")).unwrap();
        assert_eq!(spec.dest_dir, r"E:\Grab\a.example\pub\v1");
    }
}
