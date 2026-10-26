//! Links in a web page: every address the page points to, resolved against the page (and
//! its <base>), with the link text when there is one. Uses html5ever's tokenizer, so broken
//! markup is read the way browsers read it.
use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer};
use std::cell::RefCell;

#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub url: url::Url,
    pub text: Option<String>,
}

/// Attributes that hold addresses, by tag.
fn url_attrs(tag: &str) -> &'static [&'static str] {
    match tag {
        "a" | "area" => &["href"],
        "link" => &["href"],
        "img" => &["src", "data-src"],
        "source" => &["src"],
        "video" => &["src", "poster"],
        "audio" | "embed" | "track" => &["src"],
        "iframe" | "frame" => &["src"],
        "object" => &["data"],
        _ => &[],
    }
}

#[derive(Default)]
struct State {
    base: Option<url::Url>,
    links: Vec<Link>,
    /// The index of an open <a> whose text is being collected.
    open_a: Option<usize>,
    text: String,
}

struct Sink(RefCell<State>);

impl TokenSink for Sink {
    type Handle = ();
    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        let mut st = self.0.borrow_mut();
        match token {
            Token::TagToken(tag) => {
                let name = tag.name.to_string();
                if tag.kind == TagKind::EndTag {
                    if name == "a" {
                        close_a(&mut st);
                    }
                    return TokenSinkResult::Continue;
                }
                if name == "base" {
                    if let Some(href) = tag.attrs.iter().find(|a| &*a.name.local == "href") {
                        if st.base.is_some() {
                            let b = st.base.clone().unwrap();
                            st.base = b.join(href.value.trim()).ok().or(Some(b));
                        }
                    }
                    return TokenSinkResult::Continue;
                }
                if name == "a" {
                    close_a(&mut st);
                }
                for want in url_attrs(&name) {
                    let Some(attr) = tag.attrs.iter().find(|a| &*a.name.local == *want) else { continue };
                    let raw = attr.value.trim();
                    if raw.is_empty() || raw.starts_with('#') {
                        continue;
                    }
                    let Some(base) = st.base.clone() else { continue };
                    let Ok(mut u) = base.join(raw) else { continue };
                    if !matches!(u.scheme(), "http" | "https") {
                        continue;
                    }
                    u.set_fragment(None);
                    st.links.push(Link { url: u, text: None });
                    if name == "a" {
                        st.open_a = Some(st.links.len() - 1);
                        st.text.clear();
                    }
                }
                if tag.self_closing && name == "a" {
                    close_a(&mut st);
                }
            }
            Token::CharacterTokens(t) => {
                if st.open_a.is_some() && st.text.len() < 300 {
                    st.text.push_str(&t);
                }
            }
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

fn close_a(st: &mut State) {
    if let Some(i) = st.open_a.take() {
        let t = st.text.split_whitespace().collect::<Vec<_>>().join(" ");
        if !t.is_empty() {
            st.links[i].text = Some(t.chars().take(200).collect());
        }
        st.text.clear();
    }
}

/// The links of a page at `page_url`, in document order, without duplicates.
pub fn links(html: &str, page_url: &url::Url) -> Vec<Link> {
    let sink = Sink(RefCell::new(State { base: Some(page_url.clone()), ..State::default() }));
    let input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(html));
    let tok = Tokenizer::new(sink, Default::default());
    let _ = tok.feed(&input);
    tok.end();
    let mut st = tok.sink.0.into_inner();
    close_a(&mut st);
    let mut seen = std::collections::HashSet::new();
    st.links.into_iter().filter(|l| seen.insert(l.url.clone())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_and_resolves_links() {
        let page = url::Url::parse("https://site.example/docs/index.html").unwrap();
        let html = r##"<html><head><link rel=stylesheet href="/s.css"></head><body>
            <a href="guide.pdf">The <b>user</b>
               guide</a>
            <a href='../up.zip'>Up</a> <a href="#top">Top</a> <a href="mailto:x@y">Mail</a>
            <img src="img/a.png"><img data-src="//cdn.example/b.jpg">
            <video src="v.mp4" poster="p.jpg"></video><source src="a.webm">
            <a href="guide.pdf#page=2">again</a>
            <A HREF="Other.html">unclosed
            <iframe src="frame.html"></iframe>
        </body></html>"##;
        let l = links(html, &page);
        let urls: Vec<String> = l.iter().map(|x| x.url.to_string()).collect();
        assert_eq!(
            urls,
            [
                "https://site.example/s.css",
                "https://site.example/docs/guide.pdf",
                "https://site.example/up.zip",
                "https://site.example/docs/img/a.png",
                "https://cdn.example/b.jpg",
                "https://site.example/docs/v.mp4",
                "https://site.example/docs/p.jpg",
                "https://site.example/docs/a.webm",
                "https://site.example/docs/Other.html",
                "https://site.example/docs/frame.html",
            ]
        );
        assert_eq!(l[1].text.as_deref(), Some("The user guide"));
        assert_eq!(l[2].text.as_deref(), Some("Up"));
    }

    #[test]
    fn honours_base() {
        let page = url::Url::parse("https://a.example/x/page").unwrap();
        let l = links(r#"<base href="https://files.example/pub/"><a href="f.iso">f</a>"#, &page);
        assert_eq!(l[0].url.as_str(), "https://files.example/pub/f.iso");
    }
}
