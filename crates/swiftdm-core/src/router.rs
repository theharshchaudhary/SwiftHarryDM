//! Choose the engine for a URL: known media sites -> yt-dlp; files -> native engine (which
//! re-routes web pages to yt-dlp after probing). Audio presets always use yt-dlp + FFmpeg.
use regex::Regex;
use std::sync::OnceLock;

use crate::models::JobSpec;

fn media_hosts() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
            r"(?ix)^https?://(?:[a-z0-9-]+\.)*(?:
              youtube\.com|youtu\.be|youtube-nocookie\.com|vimeo\.com|dailymotion\.com|dai\.ly|twitch\.tv|
              tiktok\.com|instagram\.com|facebook\.com|fb\.watch|x\.com|twitter\.com|reddit\.com|v\.redd\.it|
              soundcloud\.com|bandcamp\.com|bilibili\.com|nicovideo\.jp|rumble\.com|streamable\.com|
              archive\.org|ted\.com|vk\.com|ok\.ru|bitchute\.com|odysee\.com|mixcloud\.com|
              pinterest\.[a-z.]+|linkedin\.com|tumblr\.com|9gag\.com|imgur\.com|kick\.com|loom\.com|
              bbc\.co\.uk|cnn\.com|nbcnews\.com|pbs\.org|arte\.tv|zdf\.de|ardmediathek\.de
            )(?:[/:?\#]|$)",
        )
        .unwrap()
    })
}

/// Paths that are clearly direct files even on media hosts (e.g. archive.org downloads).
fn direct_file() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)/download/.+\.(?:zip|7z|rar|iso|exe|msi|pdf|tar|gz)(?:\?|$)").unwrap())
}

pub fn is_media_url(url: &str) -> bool {
    media_hosts().is_match(url) && !direct_file().is_match(url)
}

pub fn route(spec: &JobSpec) -> &'static str {
    match spec.kind.as_str() {
        "http" => return "http",
        "media" => return "media",
        _ => {}
    }
    if spec.media.as_ref().is_some_and(|m| m.preset == "mp3" || m.preset == "m4a" || m.format_id.is_some()) {
        return "media";
    }
    if is_media_url(&spec.url) {
        "media"
    } else {
        "http"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::MediaOptions;
    #[test]
    fn routing() {
        let s = |u: &str| JobSpec::new(u, "x");
        assert_eq!(route(&s("https://www.youtube.com/watch?v=aqz-KE-bpKQ")), "media");
        assert_eq!(route(&s("https://youtu.be/aqz-KE-bpKQ")), "media");
        assert_eq!(route(&s("https://vimeo.com/76979871")), "media");
        assert_eq!(route(&s("https://example.com/files/setup.exe")), "http");
        assert_eq!(route(&s("https://notyoutube.com/x")), "http");
        assert_eq!(route(&s("https://archive.org/download/x/file.zip")), "http");
        let mut a = s("https://cdn.example/v.mp4");
        a.media = Some(MediaOptions::preset("mp3"));
        assert_eq!(route(&a), "media");
        a.media = Some(MediaOptions::preset("720"));
        assert_eq!(route(&a), "http");
    }
}
