//! Download queues and their schedules (the Scheduler): start time, days of the week,
//! optional stop time, simultaneous downloads, and an action when the queue finishes.
use chrono::{Datelike, Local, NaiveTime, Timelike};
use serde::{Deserialize, Serialize};

pub const MAIN_QUEUE: &str = "main";
/// The built-in synchronization queue.
pub const SYNC_QUEUE: &str = "main_sync";

/// A download queue removes files once they are done; a synchronization queue keeps finished
/// files and, each time it starts, checks them on the server and downloads changed ones again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum QueueKind {
    #[default]
    Download,
    Synchronization,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AfterAction {
    #[default]
    None,
    Exit,
    Sleep,
    Hibernate,
    Shutdown,
}

impl AfterAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            AfterAction::None => "none",
            AfterAction::Exit => "exit",
            AfterAction::Sleep => "sleep",
            AfterAction::Hibernate => "hibernate",
            AfterAction::Shutdown => "shutdown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Schedule {
    pub enabled: bool,
    /// "HH:MM" local time.
    pub start_time: String,
    /// Monday..Sunday.
    pub days: [bool; 7],
    #[serde(default)]
    pub stop_time: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Queue {
    pub id: String,
    pub name: String,
    pub max_concurrent: u32,
    pub running: bool,
    #[serde(default)]
    pub schedule: Option<Schedule>,
    #[serde(default)]
    pub after: AfterAction,
    /// Tries for each file before it is marked failed (None = Options > Connection).
    /// A file that fails goes to the end of the queue and the next one starts.
    #[serde(default)]
    pub retries: Option<u32>,
    #[serde(default)]
    pub kind: QueueKind,
}

impl Queue {
    pub fn main() -> Self {
        Queue {
            id: MAIN_QUEUE.into(),
            name: "Main queue".into(),
            max_concurrent: 2,
            running: false,
            schedule: None,
            after: AfterAction::None,
            retries: None,
            kind: QueueKind::Download,
        }
    }

    pub fn main_sync() -> Self {
        Queue { id: SYNC_QUEUE.into(), name: "Synchronization queue".into(), max_concurrent: 1, kind: QueueKind::Synchronization, ..Queue::main() }
    }
}

fn parse_hm(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s.trim(), "%H:%M").ok()
}

#[derive(Debug, PartialEq, Eq)]
pub enum Tick {
    Start,
    Stop,
    Nothing,
}

/// Decide what the schedule wants at local time `now_min` (minutes since midnight) on weekday
/// `weekday` (0 = Monday), given the minute of the previous tick, so a start or stop time is
/// acted on exactly once even if ticks are irregular.
pub fn evaluate(s: &Schedule, prev_min: u32, now_min: u32, weekday: usize) -> Tick {
    if !s.enabled || !s.days[weekday % 7] {
        return Tick::Nothing;
    }
    let crossed = |t: NaiveTime| {
        let m = t.hour() * 60 + t.minute();
        if prev_min <= now_min {
            prev_min < m && m <= now_min || (prev_min == now_min && m == now_min)
        } else {
            m > prev_min || m <= now_min
        }
    };
    if let Some(stop) = s.stop_time.as_deref().and_then(parse_hm) {
        if crossed(stop) {
            return Tick::Stop;
        }
    }
    match parse_hm(&s.start_time) {
        Some(start) if crossed(start) => Tick::Start,
        _ => Tick::Nothing,
    }
}

pub fn local_now() -> (u32, usize) {
    let n = Local::now();
    (n.hour() * 60 + n.minute(), n.weekday().num_days_from_monday() as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sched(start: &str, stop: Option<&str>) -> Schedule {
        Schedule { enabled: true, start_time: start.into(), days: [true; 7], stop_time: stop.map(Into::into) }
    }
    #[test]
    fn start_and_stop_fire_once() {
        let s = sched("02:00", Some("06:30"));
        assert_eq!(evaluate(&s, 119, 120, 0), Tick::Start);
        assert_eq!(evaluate(&s, 120, 121, 0), Tick::Nothing);
        assert_eq!(evaluate(&s, 389, 390, 0), Tick::Stop);
        assert_eq!(evaluate(&s, 100, 130, 0), Tick::Start); // a delayed tick still catches it
    }
    #[test]
    fn respects_days_and_midnight_wrap() {
        let mut s = sched("00:00", None);
        assert_eq!(evaluate(&s, 1439, 0, 2), Tick::Start);
        s.days = [false; 7];
        assert_eq!(evaluate(&s, 1439, 0, 2), Tick::Nothing);
        s.days[2] = true;
        s.enabled = false;
        assert_eq!(evaluate(&s, 1439, 0, 2), Tick::Nothing);
    }
}
