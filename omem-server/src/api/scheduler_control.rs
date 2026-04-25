use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug)]
pub struct SchedulerControl {
    pub lifecycle_paused: AtomicBool,
    pub clustering_paused: AtomicBool,
    pub lifecycle_running: AtomicBool,
    pub clustering_running: AtomicBool,
}

impl SchedulerControl {
    pub fn new() -> Self {
        Self {
            lifecycle_paused: AtomicBool::new(false),
            clustering_paused: AtomicBool::new(false),
            lifecycle_running: AtomicBool::new(false),
            clustering_running: AtomicBool::new(false),
        }
    }

    pub fn is_lifecycle_paused(&self) -> bool {
        self.lifecycle_paused.load(Ordering::Relaxed)
    }

    pub fn is_clustering_paused(&self) -> bool {
        self.clustering_paused.load(Ordering::Relaxed)
    }

    pub fn pause_lifecycle(&self) {
        self.lifecycle_paused.store(true, Ordering::Relaxed);
    }

    pub fn resume_lifecycle(&self) {
        self.lifecycle_paused.store(false, Ordering::Relaxed);
    }

    pub fn pause_clustering(&self) {
        self.clustering_paused.store(true, Ordering::Relaxed);
    }

    pub fn resume_clustering(&self) {
        self.clustering_paused.store(false, Ordering::Relaxed);
    }

    pub fn set_lifecycle_running(&self, running: bool) {
        self.lifecycle_running.store(running, Ordering::Relaxed);
    }

    pub fn set_clustering_running(&self, running: bool) {
        self.clustering_running.store(running, Ordering::Relaxed);
    }
}

pub type SharedSchedulerControl = Arc<SchedulerControl>;
