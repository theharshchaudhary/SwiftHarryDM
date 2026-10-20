//! Quality presets -> yt-dlp format selectors and post-processing arguments.
//!
//! YouTube no longer serves progressive (audio+video) HD streams, so HD presets select
//! separate video and audio streams and merge them with FFmpeg. The exact format IDs chosen
//! at probe time are pinned on the job, so a resumed download never switches quality.
use crate::models::MediaOptions;

pub const AUDIO_PRESETS: [&str; 2] = ["mp3", "m4a"];

pub fn label(preset: &str) -> &'static str {
    match preset {
        "2160" => "2160p (4K)",
        "1440" => "1440p",
        "1080" => "1080p",
        "720" => "720p",
        "480" => "480p",
        "360" => "360p",
        "mp4" => "MP4 (most compatible)",
        "mkv" => "MKV (any codec)",
        "mp3" => "MP3 audio",
        "m4a" => "M4A audio (no re-encode)",
        _ => "Best quality",
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    pub selector: String,
    pub postprocess: Vec<String>,
    pub audio_only: bool,
    pub max_height: Option<u32>,
    pub audio_codec: Option<&'static str>,
    /// yt-dlp format sorting (-S) used when choosing the formats at probe time.
    pub sort: Option<String>,
}

/// Video comes out as MP4, like IDM: among formats of the wanted quality, H.264 video and AAC
/// audio are preferred (they play everywhere); if a quality only exists as VP9 or AV1 (YouTube
/// above 1080p), that is kept and put into an MP4 file without re-encoding.
fn mp4_out() -> Vec<String> {
    args(&["--merge-output-format", "mp4", "--remux-video", "mp4"])
}

fn mp4_sort(height: Option<u32>) -> Option<String> {
    Some(match height {
        Some(h) => format!("res:{h},vcodec:h264,acodec:aac"),
        None => "res,vcodec:h264,acodec:aac".into(),
    })
}

/// "<=?" keeps formats whose height is unknown (e.g. direct links) instead of rejecting them.
fn h(height: Option<u32>) -> String {
    height.map(|x| format!("[height<=?{x}]")).unwrap_or_default()
}

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

pub fn build(o: &MediaOptions) -> Selection {
    if let Some(id) = &o.format_id {
        // An exact format picked in the format list: take it, plus the best audio if it is video-only.
        return Selection { selector: format!("{id}+ba/{id}"), postprocess: mp4_out(), audio_only: false, max_height: None, audio_codec: None, sort: Some("acodec:aac".into()) };
    }
    match o.preset.as_str() {
        "mp3" => Selection { selector: "ba/b".into(), postprocess: args(&["-x", "--audio-format", "mp3", "--audio-quality", "0"]), audio_only: true, max_height: None, audio_codec: Some("mp3"), sort: None },
        "m4a" => Selection { selector: "ba[ext=m4a]/ba/b".into(), postprocess: args(&["-x", "--audio-format", "m4a"]), audio_only: true, max_height: None, audio_codec: Some("aac"), sort: None },
        p if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() => {
            let height: u32 = p.parse().unwrap_or(1080);
            let f = h(Some(height));
            Selection { selector: format!("bv*{f}+ba/b{f}"), postprocess: mp4_out(), audio_only: false, max_height: Some(height), audio_codec: None, sort: mp4_sort(Some(height)) }
        }
        "mp4" => {
            let f = h(o.max_height);
            Selection {
                selector: format!("bv*{f}[vcodec^=avc1]+ba[acodec^=mp4a]/bv*{f}[ext=mp4]+ba[ext=m4a]/b{f}[ext=mp4]/bv*{f}+ba/b{f}"),
                postprocess: mp4_out(),
                audio_only: false,
                max_height: o.max_height,
                audio_codec: None,
                sort: None,
            }
        }
        "mkv" => {
            let f = h(o.max_height);
            Selection { selector: format!("bv*{f}+ba/b{f}"), postprocess: args(&["--merge-output-format", "mkv"]), audio_only: false, max_height: o.max_height, audio_codec: None, sort: None }
        }
        _ => {
            let f = h(o.max_height);
            Selection { selector: format!("bv*{f}+ba/b{f}"), postprocess: mp4_out(), audio_only: false, max_height: o.max_height, audio_codec: None, sort: mp4_sort(o.max_height) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presets() {
        assert_eq!(build(&MediaOptions::preset("1080")).selector, "bv*[height<=?1080]+ba/b[height<=?1080]");
        assert_eq!(build(&MediaOptions::preset("best")).selector, "bv*+ba/b");
        // Video presets come out as MP4, preferring H.264 + AAC at the chosen height.
        let p = build(&MediaOptions::preset("1080"));
        assert_eq!(p.sort.as_deref(), Some("res:1080,vcodec:h264,acodec:aac"));
        assert!(p.postprocess.windows(2).any(|w| w == ["--merge-output-format", "mp4"]));
        let a = build(&MediaOptions::preset("mp3"));
        assert!(a.audio_only && a.audio_codec == Some("mp3") && a.postprocess.contains(&"mp3".to_string()));
        assert!(build(&MediaOptions::preset("mp4")).selector.contains("avc1"));
        let exact = build(&MediaOptions { preset: "best".into(), max_height: None, format_id: Some("137".into()) });
        assert_eq!(exact.selector, "137+ba/137");
    }
}
