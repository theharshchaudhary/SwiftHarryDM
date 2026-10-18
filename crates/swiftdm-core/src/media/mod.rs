//! Media engine: yt-dlp as a separate, killable process, plus FFmpeg for merging/converting.
//!
//! probe: `yt-dlp -J -f <selector>` resolves the preset to concrete format IDs, which are
//!        pinned on the job so a resume never changes quality.
//! run:   downloads into a per-job hidden temp folder; success requires exit code 0 AND a
//!        reported output file inside that folder AND an ffprobe check.
//! Pause kills the process tree and keeps the temp folder (yt-dlp --continue resumes its
//! .part files). Nothing here bypasses DRM or access controls: protected media is refused.
pub mod errormap;
pub mod formats;
pub mod progress;
pub mod runner;
pub mod tools;
pub mod validate;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};

use crate::cancel::{CancelToken, StopReason};
use crate::engine::{EResult, Engine, EngineError, Outcome, ProbeResult, RunCtx};
use crate::errors::{ErrorCode, SdmError};
use crate::fs::long_path;
use crate::models::{Job, JobPatch, MediaOptions, ProgressSnapshot};
use progress::{Aggregator, FILE_TEMPLATE, POSTPROCESS_TEMPLATE, PROGRESS_TEMPLATE};
use runner::Line;
pub use tools::Tools;

const OUTPUT_TEMPLATE: &str = "%(title).150B.%(ext)s";
const PROBE_TIMEOUT: Duration = Duration::from_secs(180);

pub struct MediaEngine {
    tools: Mutex<Tools>,
    /// Extra yt-dlp arguments (tests).
    pub extra_args: Vec<String>,
}

/// One downloadable format, for the quality picker.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FormatInfo {
    pub id: String,
    pub ext: String,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub vcodec: Option<String>,
    pub acodec: Option<String>,
    pub size: Option<u64>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MediaInfo {
    pub title: String,
    pub extractor: String,
    pub duration: Option<f64>,
    pub thumbnail: Option<String>,
    pub formats: Vec<FormatInfo>,
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn nonempty_codec(v: &Value) -> Option<String> {
    v.as_str().filter(|c| *c != "none").map(str::to_string)
}

impl MediaEngine {
    pub fn new(tools: Tools) -> Self {
        MediaEngine { tools: Mutex::new(tools), extra_args: vec![] }
    }

    fn tools(&self) -> Tools {
        self.tools.lock().unwrap().clone()
    }

    fn ytdlp(&self) -> Result<String, SdmError> {
        self.tools()
            .ytdlp
            .map(|p| p.to_string_lossy().into_owned())
            .ok_or_else(|| SdmError::new(ErrorCode::ToolMissing, "yt-dlp was not found. Reinstall SwiftHarryDM to download videos."))
    }

    fn common(&self) -> Vec<String> {
        let t = self.tools();
        let mut a = s(&["--ignore-config", "--no-colors", "--encoding", "utf-8", "--no-playlist", "--retries", "10", "--fragment-retries", "10", "--socket-timeout", "30", "--windows-filenames", "--no-mtime"]);
        a.push("--cache-dir".into());
        a.push(crate::appdata::app_dir().join("ytcache").to_string_lossy().into_owned());
        if let Some(ff) = t.ffmpeg.as_ref().and_then(|p| p.parent()) {
            a.push("--ffmpeg-location".into());
            a.push(ff.to_string_lossy().into_owned());
        }
        if let Some((name, path)) = &t.js {
            a.push("--js-runtimes".into());
            a.push(format!("{name}:{}", path.display()));
        }
        a.extend(self.extra_args.iter().cloned());
        a
    }

    async fn json(&self, url: &str, selector: Option<&str>, cancel: &CancelToken) -> Result<Value, EngineError> {
        let mut cmd = vec![self.ytdlp()?];
        cmd.extend(self.common());
        cmd.push("-J".into());
        if let Some(sel) = selector {
            cmd.push("-f".into());
            cmd.push(sel.into());
        }
        cmd.push("--".into());
        cmd.push(url.into());
        let mut out = String::new();
        let r = runner::run(
            &cmd,
            cancel,
            |l| {
                if let Line::Out(l) = l {
                    out.push_str(&l);
                    out.push('\n');
                }
            },
            Some(PROBE_TIMEOUT),
        )
        .await
        .map_err(|e| SdmError::new(ErrorCode::ToolMissing, "yt-dlp could not be started.").detail(e.to_string()))?;
        if r.stopped {
            return Err(EngineError::Stop(cancel.reason().unwrap_or(StopReason::Pause)));
        }
        if r.timed_out {
            return Err(SdmError::new(ErrorCode::Timeout, "Reading the video information took too long.").retryable().into());
        }
        if r.code != Some(0) {
            return Err(errormap::map_error(&r.stderr_tail, r.code).into());
        }
        let info: Value = serde_json::from_str(&out).map_err(|e| SdmError::new(ErrorCode::Extractor, "yt-dlp returned unreadable video information.").detail(e.to_string()))?;
        if matches!(info["_type"].as_str(), Some("playlist" | "multi_video")) {
            return Err(SdmError::new(ErrorCode::Unsupported, "Playlists aren't supported yet. Open a single video.").into());
        }
        if info["is_live"].as_bool() == Some(true) || matches!(info["live_status"].as_str(), Some("is_live" | "is_upcoming" | "post_live")) {
            return Err(SdmError::new(ErrorCode::Unsupported, "Live streams aren't supported yet.").into());
        }
        Ok(info)
    }

    /// Title and available formats, for the quality picker.
    pub async fn inspect(&self, url: &str) -> Result<MediaInfo, String> {
        let info = self.json(url, None, &CancelToken::new()).await.map_err(|e| match e {
            EngineError::Fail(e) => e.0.message,
            EngineError::Stop(_) => "Cancelled.".into(),
        })?;
        let mut formats: Vec<FormatInfo> = info["formats"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|f| f["has_drm"].as_bool() != Some(true))
                    .filter(|f| nonempty_codec(&f["vcodec"]).is_some() || nonempty_codec(&f["acodec"]).is_some())
                    .map(|f| FormatInfo {
                        id: f["format_id"].as_str().unwrap_or("").into(),
                        ext: f["ext"].as_str().unwrap_or("").into(),
                        height: f["height"].as_u64().map(|h| h as u32),
                        fps: f["fps"].as_f64(),
                        vcodec: nonempty_codec(&f["vcodec"]),
                        acodec: nonempty_codec(&f["acodec"]),
                        size: f["filesize"].as_u64().or(f["filesize_approx"].as_u64()),
                        note: f["format_note"].as_str().unwrap_or("").into(),
                    })
                    .filter(|f| !f.id.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        formats.sort_by(|a, b| b.height.cmp(&a.height).then(b.size.cmp(&a.size)));
        Ok(MediaInfo {
            title: info["title"].as_str().unwrap_or("video").into(),
            extractor: info["extractor_key"].as_str().or(info["extractor"].as_str()).unwrap_or("").into(),
            duration: info["duration"].as_f64(),
            thumbnail: info["thumbnail"].as_str().map(str::to_string),
            formats,
        })
    }

    pub fn temp_dir_for(job: &Job) -> String {
        Path::new(job.dest_dir()).join(format!(".{}.sdmtmp", &job.id[..8])).to_string_lossy().into_owned()
    }

    fn options(job: &Job) -> MediaOptions {
        job.spec.media.clone().unwrap_or_else(|| MediaOptions::preset(&job.spec.format_label))
    }
}

#[async_trait::async_trait]
impl Engine for MediaEngine {
    fn name(&self) -> &'static str {
        "media"
    }

    async fn probe(&self, job: &Job, ctx: &RunCtx) -> EResult<ProbeResult> {
        let opts = Self::options(job);
        let sel = formats::build(&opts);
        let info = self.json(&job.spec.url, Some(&sel.selector), &ctx.cancel).await?;
        let single = [info.clone()];
        let chosen: &[Value] = info["requested_formats"].as_array().map(|v| v.as_slice()).unwrap_or(&single);
        if chosen.iter().any(|f| f["has_drm"].as_bool() == Some(true)) {
            return Err(SdmError::new(ErrorCode::Unsupported, "This video is protected by DRM and can't be downloaded.").into());
        }
        if (chosen.len() > 1 || sel.audio_only) && self.tools().ffmpeg.is_none() {
            return Err(SdmError::new(ErrorCode::ToolMissing, "FFmpeg is needed for this quality but was not found. Reinstall SwiftHarryDM.").into());
        }
        let heights: Vec<u32> = chosen.iter().filter(|f| nonempty_codec(&f["vcodec"]).is_some()).filter_map(|f| f["height"].as_u64().map(|h| h as u32)).collect();
        let has_audio = chosen.iter().any(|f| nonempty_codec(&f["acodec"]).is_some());
        let sizes: HashMap<String, Option<u64>> = chosen
            .iter()
            .map(|f| (f["format_id"].as_str().unwrap_or("").to_string(), f["filesize"].as_u64().or(f["filesize_approx"].as_u64())))
            .collect();
        let format_id = info["format_id"].as_str().ok_or_else(|| SdmError::new(ErrorCode::Extractor, "yt-dlp did not report a format for this video."))?;
        let height = heights.iter().max().copied();
        let total: Option<u64> = sizes.values().copied().sum();
        let media = json!({
            "format_id": format_id, "preset": opts.preset, "id": info["id"], "extractor": info["extractor_key"],
            "height": height, "has_audio": has_audio, "duration": info["duration"], "audio_only": sel.audio_only,
            "max_height": sel.max_height, "audio_codec": sel.audio_codec, "sizes": sizes,
        });
        let title = info["title"].as_str().or(info["id"].as_str()).unwrap_or("video").to_string();
        ctx.reporter.log(
            &job.id,
            "info",
            &format!("{}: {title} - format {format_id}{}", info["extractor_key"].as_str().unwrap_or("media"), height.map(|h| format!(" ({h}p)")).unwrap_or_default()),
        );
        Ok(ProbeResult {
            patch: JobPatch { title: Some(title), media: Some(media), final_url: Some(job.spec.url.clone()), total_bytes: total.map(Some), ..Default::default() },
            desired_name: None,
            reroute: None,
        })
    }

    async fn run(&self, job: &mut Job, ctx: &RunCtx) -> EResult<Outcome> {
        let media = job.media.clone().ok_or_else(|| SdmError::new(ErrorCode::Internal, "Internal error: media job was not probed."))?;
        let tmp = job.temp_path.clone().unwrap_or_else(|| Self::temp_dir_for(job));
        std::fs::create_dir_all(long_path(&tmp)).map_err(|e| crate::errors::from_io(&e, &tmp))?;
        if job.temp_path.as_deref() != Some(tmp.as_str()) {
            job.temp_path = Some(tmp.clone());
            ctx.reporter.update(&job.id, JobPatch { temp_path: Some(tmp.clone()), ..Default::default() });
        }
        let sel = formats::build(&Self::options(job));
        let mut cmd = vec![self.ytdlp()?];
        cmd.extend(self.common());
        cmd.extend(["-f".into(), media["format_id"].as_str().unwrap_or("b").to_string()]);
        cmd.extend(sel.postprocess.iter().cloned());
        cmd.extend(s(&["-o", OUTPUT_TEMPLATE, "--paths"]));
        cmd.push(format!("home:{tmp}"));
        cmd.push("--paths".into());
        cmd.push(format!("temp:{tmp}"));
        cmd.extend(s(&["--continue", "--no-overwrites", "--newline", "--progress", "--progress-template", PROGRESS_TEMPLATE, "--progress-template", POSTPROCESS_TEMPLATE, "--print", FILE_TEMPLATE, "--no-simulate"]));
        if let Some(r) = &job.spec.referer {
            cmd.push("--referer".into());
            cmd.push(r.clone());
        }
        if let Some(l) = job.spec.speed_limit.filter(|l| *l > 0) {
            cmd.push("--limit-rate".into());
            cmd.push(l.to_string());
        }
        cmd.push("--".into());
        cmd.push(job.spec.url.clone());

        let sizes: HashMap<String, Option<u64>> = media["sizes"].as_object().map(|m| m.iter().map(|(k, v)| (k.clone(), v.as_u64())).collect()).unwrap_or_default();
        let mut agg = Aggregator::new(sizes);
        let mut outputs: Vec<String> = vec![];
        let mut post = false;
        let mut last_report = std::time::Instant::now() - Duration::from_secs(1);
        let id = job.id.clone();
        let r = runner::run(
            &cmd,
            &ctx.cancel,
            |l| {
                let line = match &l {
                    Line::Out(s) | Line::Err(s) => s.as_str(),
                };
                if let Some(t) = progress::parse_progress(line) {
                    agg.feed(&t);
                    if last_report.elapsed() >= Duration::from_millis(250) {
                        last_report = std::time::Instant::now();
                        let (done, total, speed, eta) = agg.figures();
                        ctx.reporter.progress(ProgressSnapshot {
                            job_id: id.clone(),
                            downloaded: done,
                            total,
                            speed,
                            eta,
                            phase: if post { "postprocessing" } else { "downloading" }.into(),
                            segments: vec![],
                        });
                    }
                } else if let Some(f) = progress::parse_file(line) {
                    outputs.push(f);
                } else if progress::is_postprocess(line) && !post {
                    post = true;
                    ctx.reporter.phase(&id, "postprocessing");
                }
            },
            None,
        )
        .await
        .map_err(|e| SdmError::new(ErrorCode::ToolMissing, "yt-dlp could not be started.").detail(e.to_string()))?;
        if r.stopped {
            return Err(ctx.stop_err());
        }
        if r.code != Some(0) {
            return Err(errormap::map_error(&r.stderr_tail, r.code).into());
        }
        let path = outputs.pop().ok_or_else(|| SdmError::new(ErrorCode::Postprocess, "yt-dlp finished but reported no output file.").detail(r.stderr_tail.join("\n")))?;
        if !long_path(&path).is_file() {
            return Err(SdmError::new(ErrorCode::Postprocess, "yt-dlp reported an output file that does not exist.").detail(&path).into());
        }
        let inside = Path::new(&path).parent().map(|p| p.to_string_lossy().to_lowercase()) == Some(Path::new(&tmp).to_string_lossy().to_lowercase());
        if !inside {
            return Err(SdmError::new(ErrorCode::Postprocess, "yt-dlp wrote outside the download's temporary folder.").detail(&path).into());
        }
        let verified = match self.tools().ffprobe {
            Some(fp) => {
                let e = validate::Expect {
                    audio_only: media["audio_only"].as_bool().unwrap_or(false),
                    expect_audio: media["has_audio"].as_bool().unwrap_or(false) && !media["audio_only"].as_bool().unwrap_or(false),
                    expect_height: media["height"].as_u64().map(|h| h as u32),
                    max_height: media["max_height"].as_u64().map(|h| h as u32),
                    duration: media["duration"].as_f64(),
                    audio_codec: media["audio_codec"].as_str().map(str::to_string),
                };
                validate::validate(&fp.to_string_lossy(), &path, &e).await?;
                true
            }
            None => {
                ctx.reporter.log(&job.id, "warning", "ffprobe was not found, so the media file could not be checked.");
                false
            }
        };
        let size = std::fs::metadata(long_path(&path)).map(|m| m.len()).unwrap_or(0);
        Ok(Outcome::Completed { artifact: path, size, verified })
    }

    fn discard(&self, job: &Job) {
        for d in [job.temp_path.clone(), Some(Self::temp_dir_for(job))].into_iter().flatten() {
            let _ = std::fs::remove_dir_all(long_path(&d));
        }
    }

    fn cleanup(&self, job: &Job) {
        self.discard(job);
    }
}
