use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};

/// A single startup-phase timing mark.  Durations are measured from the
/// process-entry instant so marks can be compared across runs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StartupMark {
    pub name: String,
    /// Backend process-relative time. Frontend marks use bridge receipt time,
    /// keeping every value on one monotonic clock.
    pub elapsed_ms: u64,
    /// Browser navigation-relative time, retained as a separate diagnostic
    /// rather than being mixed into the backend timeline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frontend_elapsed_ms: Option<u64>,
}

static START_INSTANT: OnceLock<std::time::Instant> = OnceLock::new();
static MARKS: Mutex<Vec<StartupMark>> = Mutex::new(Vec::new());

fn start() -> &'static std::time::Instant {
    START_INSTANT.get_or_init(std::time::Instant::now)
}

/// Record a startup mark relative to process entry.
pub fn record(name: &str) {
    let elapsed = start().elapsed().as_millis() as u64;
    let mut marks = MARKS.lock().unwrap();
    marks.push(StartupMark {
        name: name.to_string(),
        elapsed_ms: elapsed,
        frontend_elapsed_ms: None,
    });
}

/// Return all recorded startup marks.
pub fn get_all() -> Vec<StartupMark> {
    MARKS.lock().unwrap().clone()
}

/// Record a frontend-supplied mark at bridge receipt time.
#[tauri::command]
pub fn record_startup_mark(name: String, frontend_elapsed_ms: u64) {
    let elapsed_ms = start().elapsed().as_millis() as u64;
    let mut marks = MARKS.lock().unwrap();
    marks.push(StartupMark {
        name,
        elapsed_ms,
        frontend_elapsed_ms: Some(frontend_elapsed_ms),
    });
}

/// Return all startup marks for the current process.
#[tauri::command]
pub fn get_startup_marks() -> Vec<StartupMark> {
    get_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_get() {
        // This test may see marks from other tests in the same process; just
        // verify that recording adds at least one mark.
        let before = get_all().len();
        record("test_mark");
        let after = get_all().len();
        assert_eq!(after, before + 1);
    }
}
