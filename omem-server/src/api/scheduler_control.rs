use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Debug)]
pub struct SchedulerControl {
    pub lifecycle_paused: AtomicBool,
    pub lifecycle_running: AtomicBool,
    pub last_run_at: AtomicI64,
    pub interval_secs: AtomicU64,
}

impl Default for SchedulerControl {
    fn default() -> Self {
        Self::new()
    }
}

impl SchedulerControl {
    pub fn new() -> Self {
        Self {
            lifecycle_paused: AtomicBool::new(false),
            lifecycle_running: AtomicBool::new(false),
            last_run_at: AtomicI64::new(0),
            interval_secs: AtomicU64::new(0),
        }
    }

    pub fn with_interval(self, secs: u64) -> Self {
        self.interval_secs.store(secs, Ordering::Relaxed);
        self
    }

    pub fn is_lifecycle_paused(&self) -> bool {
        self.lifecycle_paused.load(Ordering::Relaxed)
    }

    pub fn pause_lifecycle(&self) {
        self.lifecycle_paused.store(true, Ordering::Relaxed);
    }

    pub fn resume_lifecycle(&self) {
        self.lifecycle_paused.store(false, Ordering::Relaxed);
    }

    pub fn set_lifecycle_running(&self, running: bool) {
        self.lifecycle_running.store(running, Ordering::Relaxed);
    }

    pub fn record_run(&self) {
        let now = chrono::Utc::now().timestamp();
        self.last_run_at.store(now, Ordering::Relaxed);
    }

    pub fn last_run_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        let ts = self.last_run_at.load(Ordering::Relaxed);
        if ts == 0 {
            None
        } else {
            chrono::DateTime::from_timestamp(ts, 0)
        }
    }

    pub fn interval_secs(&self) -> u64 {
        self.interval_secs.load(Ordering::Relaxed)
    }

    pub fn next_run_eta(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        let last = self.last_run_at()?;
        let interval = self.interval_secs();
        if interval == 0 {
            return None;
        }
        Some(last + chrono::Duration::seconds(interval as i64))
    }
}

pub type SharedSchedulerControl = Arc<SchedulerControl>;
