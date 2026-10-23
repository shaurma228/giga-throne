//! Pure decisions of the sidecar supervisor: what an exit code means, how long
//! to back off and when to give up. No I/O here, so it is unit-tested directly.

use std::time::{Duration, Instant};

/// Longest pause between two starts.
pub const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Failed starts are counted over this window.
pub const FAILURE_WINDOW: Duration = Duration::from_secs(120);

/// This many failed starts inside [`FAILURE_WINDOW`] end the supervision.
pub const MAX_FAILED_STARTS: usize = 5;

/// An instance that stayed ready this long resets the failure count.
pub const STABLE_AFTER: Duration = Duration::from_secs(60);

/// What the supervisor does after Throne exited on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExitDecision {
    /// Throne asked for a restart (status 75): start it again at once.
    RestartNow,
    /// The start failed for a reason that may pass: retry after backoff.
    Backoff,
    /// Do not retry. The text is user-readable.
    Fail(String),
}

/// Maps the exit code of Throne (see `docs/throne-api/api.md`, "Exit statuses")
/// to a decision. `None` means the process was killed by a signal.
pub fn on_exit(code: Option<i32>) -> ExitDecision {
    match code {
        Some(75) => ExitDecision::RestartNow,
        Some(3) => ExitDecision::Fail("каталог данных занят другим Throne".to_owned()),
        Some(64) => ExitDecision::Fail("Throne отклонил параметры запуска (код 64)".to_owned()),
        Some(78) => ExitDecision::Fail("ошибка конфигурации Throne (код 78)".to_owned()),
        // Throne exits with 0 only when it was asked to stop, and we did not ask.
        Some(0) => ExitDecision::Fail("движок остановлен извне".to_owned()),
        _ => ExitDecision::Backoff,
    }
}

/// Pause before retry number `retry_index + 1`: 1 s, 2 s, 4 s, ... capped at [`MAX_BACKOFF`].
pub fn backoff(retry_index: u32) -> Duration {
    let secs = 1u64 << retry_index.min(5);
    Duration::from_secs(secs).min(MAX_BACKOFF)
}

/// Failed starts seen inside the sliding [`FAILURE_WINDOW`].
#[derive(Debug, Default)]
pub struct FailureWindow {
    starts: Vec<Instant>,
}

impl FailureWindow {
    /// Records a failed start at `now` and returns how many are in the window.
    pub fn record(&mut self, now: Instant) -> usize {
        self.starts
            .retain(|at| now.saturating_duration_since(*at) <= FAILURE_WINDOW);
        self.starts.push(now);
        self.starts.len()
    }

    /// Forgets all failed starts.
    pub fn reset(&mut self) {
        self.starts.clear();
    }

    /// Number of failed starts in the window.
    pub fn count(&self) -> usize {
        self.starts.len()
    }

    /// Whether the budget of failed starts is used up.
    pub fn exhausted(&self) -> bool {
        self.starts.len() >= MAX_FAILED_STARTS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_map_to_decisions() {
        assert_eq!(on_exit(Some(75)), ExitDecision::RestartNow);
        assert!(matches!(on_exit(Some(3)), ExitDecision::Fail(_)));
        assert!(matches!(on_exit(Some(64)), ExitDecision::Fail(_)));
        assert!(matches!(on_exit(Some(78)), ExitDecision::Fail(_)));
        assert!(matches!(on_exit(Some(0)), ExitDecision::Fail(_)));
        assert_eq!(on_exit(Some(1)), ExitDecision::Backoff);
        assert_eq!(on_exit(Some(70)), ExitDecision::Backoff);
        assert_eq!(on_exit(Some(71)), ExitDecision::Backoff);
        assert_eq!(on_exit(None), ExitDecision::Backoff);
    }

    #[test]
    fn backoff_doubles_and_caps() {
        let seconds: Vec<u64> = (0..8).map(|i| backoff(i).as_secs()).collect();
        assert_eq!(seconds, vec![1, 2, 4, 8, 16, 30, 30, 30]);
    }

    #[test]
    fn window_counts_and_expires() {
        let start = Instant::now();
        let mut window = FailureWindow::default();
        assert_eq!(window.record(start), 1);
        assert_eq!(window.record(start + Duration::from_secs(10)), 2);
        assert!(!window.exhausted());
        // The first two starts are older than the window by now.
        let later = start + FAILURE_WINDOW + Duration::from_secs(20);
        assert_eq!(window.record(later), 1);
        assert_eq!(window.count(), 1);
        window.reset();
        assert_eq!(window.count(), 0);
    }

    #[test]
    fn five_quick_failures_exhaust_the_budget() {
        let start = Instant::now();
        let mut window = FailureWindow::default();
        for i in 0..MAX_FAILED_STARTS as u64 {
            window.record(start + Duration::from_secs(i));
        }
        assert!(window.exhausted());
    }
}
