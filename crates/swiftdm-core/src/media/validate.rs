//! Verify finished media files with ffprobe before they are reported as complete.
use serde_json::Value;

use crate::cancel::CancelToken;
use crate::errors::{ErrorCode, SdmError};

pub struct Expect {
    pub audio_only: bool,
    pub expect_audio: bool,
    pub expect_height: Option<u32>,
    pub max_height: Option<u32>,
    pub duration: Option<f64>,
    pub audio_codec: Option<String>,
}

/// Problems found in an ffprobe report (empty = the file is what was asked for).
pub fn check(info: &Value, e: &Expect) -> Vec<String> {
    let streams = info["streams"].as_array().cloned().unwrap_or_default();
    let is_pic = |s: &Value| s["disposition"]["attached_pic"].as_i64().unwrap_or(0) == 1;
    let video: Vec<&Value> = streams.iter().filter(|s| s["codec_type"] == "video" && !is_pic(s)).collect();
    let audio: Vec<&Value> = streams.iter().filter(|s| s["codec_type"] == "audio").collect();
    let mut p = vec![];
    if e.audio_only {
        if audio.is_empty() {
            p.push("no audio stream".to_string());
        }
        if let (Some(want), Some(a)) = (&e.audio_codec, audio.first()) {
            let got = a["codec_name"].as_str().unwrap_or("");
            if got != want {
                p.push(format!("audio codec is {got}, expected {want}"));
            }
        }
    } else {
        if video.is_empty() {
            p.push("no video stream".into());
        } else {
            let h = video.iter().map(|s| s["height"].as_u64().unwrap_or(0) as u32).max().unwrap_or(0);
            if e.max_height.is_some_and(|m| h > m) {
                p.push(format!("video is {h}p, above the requested {}p", e.max_height.unwrap()));
            }
            if e.expect_height.is_some_and(|x| h != x) {
                p.push(format!("video is {h}p, expected {}p", e.expect_height.unwrap()));
            }
        }
        if e.expect_audio && audio.is_empty() {
            p.push("no audio stream".into());
        }
    }
    let got = info["format"]["duration"].as_str().and_then(|d| d.parse::<f64>().ok());
    if let (Some(want), Some(got)) = (e.duration, got) {
        if (got - want).abs() > f64::max(2.0, want * 0.02) {
            p.push(format!("duration is {got:.1}s, expected about {want:.1}s"));
        }
    }
    p
}

pub async fn validate(ffprobe: &str, path: &str, e: &Expect) -> Result<(), SdmError> {
    let cmd: Vec<String> = [ffprobe, "-v", "error", "-show_streams", "-show_format", "-of", "json", path].iter().map(|s| s.to_string()).collect();
    let mut out = String::new();
    let r = super::runner::run(
        &cmd,
        &CancelToken::new(),
        |l| {
            if let super::runner::Line::Out(l) = l {
                out.push_str(&l);
                out.push('\n');
            }
        },
        Some(std::time::Duration::from_secs(120)),
    )
    .await
    .map_err(|err| SdmError::new(ErrorCode::ToolMissing, "ffprobe could not be started.").detail(err.to_string()))?;
    if r.code != Some(0) {
        return Err(SdmError::new(ErrorCode::Postprocess, "The downloaded media file is unreadable.").detail(r.stderr_tail.join("\n")));
    }
    let info: Value = serde_json::from_str(&out).map_err(|err| SdmError::new(ErrorCode::Postprocess, "ffprobe returned unreadable output.").detail(err.to_string()))?;
    let problems = check(&info, e);
    if problems.is_empty() {
        Ok(())
    } else {
        Err(SdmError::new(ErrorCode::Postprocess, format!("The downloaded media failed verification: {}.", problems.join("; "))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn checks() {
        let info = json!({ "streams": [{ "codec_type": "video", "height": 720 }, { "codec_type": "audio", "codec_name": "aac" }], "format": { "duration": "60.0" } });
        let e = Expect { audio_only: false, expect_audio: true, expect_height: Some(720), max_height: Some(1080), duration: Some(60.5), audio_codec: None };
        assert!(check(&info, &e).is_empty());
        let e2 = Expect { expect_height: Some(1080), duration: Some(120.0), ..e };
        assert_eq!(check(&info, &e2).len(), 2);
        let audio = Expect { audio_only: true, expect_audio: true, expect_height: None, max_height: None, duration: None, audio_codec: Some("mp3".into()) };
        assert_eq!(check(&info, &audio), vec!["audio codec is aac, expected mp3".to_string()]);
    }
}
