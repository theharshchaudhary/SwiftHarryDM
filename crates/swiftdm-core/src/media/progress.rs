//! Parse yt-dlp's machine-readable progress lines (from our --progress-template) and combine
//! the video and audio streams into one figure for the job.
use std::collections::HashMap;

pub const PROGRESS_PREFIX: &str = "SDMP ";
pub const FILE_PREFIX: &str = "SDMF ";
pub const POSTPROCESS_PREFIX: &str = "SDMQ ";
/// Fields separated by '|'; yt-dlp prints "NA" for missing values.
pub const PROGRESS_TEMPLATE: &str = "download:SDMP %(info.format_id)s|%(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s";
pub const FILE_TEMPLATE: &str = "after_move:SDMF %(filepath)j";
pub const POSTPROCESS_TEMPLATE: &str = "postprocess:SDMQ %(progress.status)s|%(progress.postprocessor)s";
const POSTPROCESS_MARKERS: [&str; 9] = [
    "[Merger]", "[ExtractAudio]", "[VideoRemuxer]", "[VideoConvertor]", "[FixupM3u8]", "[FixupM4a]", "[FixupStretched]", "[FixupDuplicateMoov]", "[FixupTimestamp]",
];

#[derive(Debug, Clone, PartialEq)]
pub struct Tick {
    pub format_id: String,
    pub status: String,
    pub downloaded: Option<f64>,
    pub total: Option<f64>,
    pub estimate: Option<f64>,
    pub speed: Option<f64>,
    pub eta: Option<f64>,
}

fn num(v: &str) -> Option<f64> {
    v.trim().parse::<f64>().ok().filter(|f| *f >= 0.0 && f.is_finite())
}

pub fn parse_progress(line: &str) -> Option<Tick> {
    let rest = line.strip_prefix(PROGRESS_PREFIX)?;
    let p: Vec<&str> = rest.split('|').collect();
    if p.len() != 7 {
        return None;
    }
    Some(Tick { format_id: p[0].into(), status: p[1].into(), downloaded: num(p[2]), total: num(p[3]), estimate: num(p[4]), speed: num(p[5]), eta: num(p[6]) })
}

pub fn parse_file(line: &str) -> Option<String> {
    serde_json::from_str::<String>(line.strip_prefix(FILE_PREFIX)?).ok()
}

pub fn is_postprocess(line: &str) -> bool {
    line.starts_with(POSTPROCESS_PREFIX) || POSTPROCESS_MARKERS.iter().any(|m| line.starts_with(m))
}

/// Combine per-stream progress (video + audio downloads) into one job figure.
#[derive(Debug, Default)]
pub struct Aggregator {
    expected: HashMap<String, Option<u64>>,
    done: HashMap<String, f64>,
    total: HashMap<String, f64>,
    speed: Option<f64>,
}

impl Aggregator {
    pub fn new(expected: HashMap<String, Option<u64>>) -> Self {
        Aggregator { expected, ..Default::default() }
    }
    pub fn feed(&mut self, t: &Tick) {
        if let Some(d) = t.downloaded {
            self.done.insert(t.format_id.clone(), d);
        }
        if let Some(tot) = t.total.or(t.estimate).filter(|x| *x > 0.0) {
            self.total.insert(t.format_id.clone(), tot);
        }
        if t.status == "finished" {
            if let Some(d) = self.done.get(&t.format_id).copied() {
                self.total.insert(t.format_id.clone(), d);
            }
        }
        self.speed = t.speed;
    }
    /// (downloaded, total, speed, eta)
    pub fn figures(&self) -> (u64, Option<u64>, Option<f64>, Option<f64>) {
        let mut ids: Vec<&String> = self.expected.keys().chain(self.done.keys()).chain(self.total.keys()).collect();
        ids.sort();
        ids.dedup();
        let downloaded = self.done.values().sum::<f64>() as u64;
        let totals: Vec<Option<f64>> = ids.iter().map(|i| self.total.get(*i).copied().or(self.expected.get(*i).copied().flatten().map(|x| x as f64))).collect();
        let total = (!totals.is_empty() && totals.iter().all(Option::is_some)).then(|| totals.iter().map(|t| t.unwrap()).sum::<f64>() as u64);
        let eta = match (total, self.speed) {
            (Some(t), Some(s)) if s > 0.0 => Some(t.saturating_sub(downloaded) as f64 / s),
            _ => None,
        };
        (downloaded, total, self.speed, eta)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_and_aggregates() {
        let t = parse_progress("SDMP 137|downloading|1000|NA|4000|500.5|6").unwrap();
        assert_eq!((t.downloaded, t.total, t.estimate, t.speed), (Some(1000.0), None, Some(4000.0), Some(500.5)));
        assert!(parse_progress("SDMP 1|2").is_none());
        assert_eq!(parse_file(r#"SDMF "C:\\d\\a b.mp4""#).as_deref(), Some(r"C:\d\a b.mp4"));
        assert!(is_postprocess("[Merger] Merging formats into x.mp4") && is_postprocess("SDMQ started|Merger"));
        let mut a = Aggregator::new(HashMap::from([("137".into(), Some(4000)), ("140".into(), Some(1000))]));
        a.feed(&t);
        let (d, tot, _, eta) = a.figures();
        assert_eq!((d, tot), (1000, Some(5000)));
        assert!(eta.unwrap() > 7.0);
        a.feed(&parse_progress("SDMP 137|finished|4100|NA|NA|NA|NA").unwrap());
        assert_eq!(a.figures().1, Some(5100));
    }
}
