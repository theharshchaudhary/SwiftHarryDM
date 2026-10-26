//! Turn yt-dlp error output into actionable messages. Unknown errors keep yt-dlp's text.
use regex::Regex;
use std::sync::OnceLock;

use crate::errors::{ErrorCode, SdmError};

type Rule = (Regex, ErrorCode, &'static str, bool);

fn rules() -> &'static [Rule] {
    static R: OnceLock<Vec<Rule>> = OnceLock::new();
    R.get_or_init(|| {
        let r = |p: &str, c, m, retry| (Regex::new(&format!("(?i){p}")).unwrap(), c, m, retry);
        use ErrorCode::*;
        vec![
            r("DRM", Unsupported, "This video is protected by DRM and can't be downloaded.", false),
            r("confirm you(?:'|’)re not a bot|not a bot", AuthRequired, "The site asked to confirm you're not a bot. Try again later, or send the video from your browser so its cookies are used.", false),
            r("Sign in to confirm your age|age[- ]restricted|inappropriate for some users", AuthRequired, "This video is age-restricted. Send it from your browser while signed in so its cookies are used.", false),
            r("members[- ]only|Join this channel", AuthRequired, "This video is for channel members only.", false),
            r("Private video", Unavailable, "This video is private.", false),
            r(r"available in your (?:country|region)|geo[- ]?(?:restrict|block)", GeoBlocked, "This video isn't available in your country.", false),
            r("Video unavailable|has been removed|no longer available|account associated with this video has been terminated", Unavailable, "This video is unavailable (removed or blocked).", false),
            r("Requested format is not available", FormatGone, "The selected quality is not available for this video. Choose another quality.", false),
            r("Unsupported URL", Unsupported, "This website or address is not supported.", false),
            r(r"ffmpeg (?:is )?not (?:found|installed)|ffprobe (?:is )?not found|Postprocessing: .*ffmpeg", ToolMissing, "FFmpeg is missing or not working. Reinstall Keepfetch.", false),
            r(r"No space left|\[Errno 28\]|disk is full", DiskFull, "The disk is full. Free some space, then resume.", false),
            r("HTTP Error 429|Too Many Requests", RateLimited, "The site is limiting requests. Retrying later.", true),
            r("HTTP Error 403", Forbidden, "The site refused the download (403). It will be retried.", true),
            r(r"HTTP Error 5\d\d", HttpStatus, "The site had a temporary problem. Retrying.", true),
            r("timed out|Connection reset|getaddrinfo failed|Failed to resolve|Unable to download webpage|Remote end closed|IncompleteRead|ConnectionError|Network is unreachable", Network, "The connection failed. Retrying.", true),
            r("CERTIFICATE_VERIFY_FAILED|certificate verify failed", Tls, "The secure connection could not be verified (certificate problem).", false),
        ]
    })
}

pub fn map_error(stderr: &[String], code: Option<i32>) -> SdmError {
    let errors: Vec<&String> = stderr.iter().filter(|l| l.starts_with("ERROR:")).collect();
    let text = if errors.is_empty() {
        stderr.iter().rev().take(10).rev().cloned().collect::<Vec<_>>().join("\n")
    } else {
        errors.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n")
    };
    for (rx, c, msg, retry) in rules() {
        if rx.is_match(&text) {
            let e = SdmError::new(*c, *msg).detail(&text);
            return if *retry { e.retryable() } else { e };
        }
    }
    let last: String = errors
        .last()
        .map(|e| e.trim_start_matches("ERROR:").trim().to_string())
        .or_else(|| stderr.last().cloned())
        .unwrap_or_default()
        .chars()
        .take(300)
        .collect();
    let msg = if last.is_empty() {
        format!("The media download failed (yt-dlp exit code {}).", code.map(|c| c.to_string()).unwrap_or_else(|| "unknown".into()))
    } else {
        format!("The media download failed: {}", crate::logging::redact(&last))
    };
    SdmError::new(ErrorCode::Extractor, msg).detail(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn m(lines: &[&str]) -> SdmError {
        map_error(&lines.iter().map(|s| s.to_string()).collect::<Vec<_>>(), Some(1))
    }
    #[test]
    fn maps_known_errors() {
        assert_eq!(m(&["ERROR: [youtube] x: Private video. Sign in"]).code(), ErrorCode::Unavailable);
        assert_eq!(m(&["ERROR: [youtube] x: Sign in to confirm you're not a bot"]).code(), ErrorCode::AuthRequired);
        let e = m(&["ERROR: unable to download video data: HTTP Error 503: Service Unavailable"]);
        assert!(e.code() == ErrorCode::HttpStatus && e.info().retryable);
        let e = m(&["ERROR: something odd happened"]);
        assert!(e.code() == ErrorCode::Extractor && e.info().message.contains("something odd"));
        assert!(m(&[]).info().message.contains("exit code 1"));
    }
}
