//! The job state machine. Every state change goes through `check`.
use crate::models::JobState::{self, *};

pub fn allowed(src: JobState) -> &'static [JobState] {
    match src {
        Queued => &[Starting, Paused, Cancelled],
        Starting => &[Downloading, RetryWait, Failed, Pausing, Cancelling, Paused, Queued],
        Downloading => &[Postprocessing, Finalizing, RetryWait, Failed, Pausing, Cancelling, Paused, Queued],
        Postprocessing => &[Finalizing, RetryWait, Failed, Pausing, Cancelling, Paused, Queued],
        // Finalizing is a short file move; it cannot be paused or cancelled midway.
        Finalizing => &[Completed, Failed],
        // A worker may still finish (or fail) after a pause was requested.
        Pausing => &[Paused, Finalizing, Failed, Cancelling],
        Paused => &[Queued, Cancelled],
        RetryWait => &[Queued, Paused, Cancelled],
        Cancelling => &[Cancelled],
        Failed => &[Queued, Cancelled],
        Completed | Cancelled => &[],
    }
}

pub fn can(src: JobState, dst: JobState) -> bool {
    allowed(src).contains(&dst)
}

#[derive(Debug, thiserror::Error)]
#[error("invalid transition {0:?} -> {1:?}")]
pub struct InvalidTransition(pub JobState, pub JobState);

pub fn check(src: JobState, dst: JobState) -> Result<(), InvalidTransition> {
    if can(src, dst) {
        Ok(())
    } else {
        Err(InvalidTransition(src, dst))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn terminal_states_have_no_exits() {
        assert!(allowed(Completed).is_empty() && allowed(Cancelled).is_empty());
    }

    #[test]
    fn every_state_reachable_from_queued() {
        let mut seen = HashSet::from([Queued]);
        let mut todo = vec![Queued];
        while let Some(s) = todo.pop() {
            for &n in allowed(s) {
                if seen.insert(n) {
                    todo.push(n);
                }
            }
        }
        assert_eq!(seen.len(), JobState::ALL.len());
    }

    #[test]
    fn completed_only_via_finalizing() {
        for s in JobState::ALL {
            if allowed(s).contains(&Completed) {
                assert_eq!(s, Finalizing);
            }
        }
    }

    #[test]
    fn specific_transitions() {
        for (a, b, ok) in [
            (Queued, Starting, true),
            (Starting, Downloading, true),
            (Downloading, Finalizing, true),
            (Finalizing, Completed, true),
            (Completed, Queued, false),
            (Cancelled, Queued, false),
            (Queued, Completed, false),
            (Downloading, Completed, false),
            (Paused, Downloading, false),
            (Finalizing, Cancelling, false),
            (Failed, Queued, true),
            (Pausing, Cancelling, true),
        ] {
            assert_eq!(can(a, b), ok, "{a:?}->{b:?}");
            assert_eq!(check(a, b).is_ok(), ok);
        }
    }
}
