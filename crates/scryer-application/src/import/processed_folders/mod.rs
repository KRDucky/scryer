//! Processed-folder configuration and the in-process runtime a scan shares.
//!
//! Scryer only reads a processed folder. Imports copy. Nothing in the folder
//! is moved, linked, renamed, or deleted.

mod actions;
mod config;
mod importer;
mod scan;
mod stability;
mod validation;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::Mutex;

pub use actions::{ProcessedFolderPreview, ProcessedFolderScanReport};
pub use config::{
    CreateProcessedFolder, ProcessedFolderEntryConnection, ProcessedFolderEntryView,
    ProcessedFolderView, UpdateProcessedFolder,
};
pub use validation::MarkerValueUpdate;

/// Clock the ledger uses for eligibility and claim timestamps.
///
/// Production uses [`UtcProcessedFolderClock`]. Tests pass a closure.
pub trait ProcessedFolderClock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

/// `Utc::now`.
pub struct UtcProcessedFolderClock;

impl ProcessedFolderClock for UtcProcessedFolderClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

impl<F> ProcessedFolderClock for F
where
    F: Fn() -> DateTime<Utc> + Send + Sync,
{
    fn now(&self) -> DateTime<Utc> {
        self()
    }
}

/// In-process scan locks and the set of entry ids a live task owns.
///
/// An orphan is a row still marked `importing` whose id is not in `in_flight`.
/// No wall-clock timeout is used, so a long copy is not stolen while its task
/// lives. Clones share the same sets.
#[derive(Clone)]
pub struct ProcessedFolderRuntime {
    // Core inserts an entry id before the claim and removes it after the ledger write.
    #[allow(dead_code)]
    in_flight: Arc<Mutex<HashSet<String>>>,
    scan_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    clock: Arc<dyn ProcessedFolderClock>,
}

impl Default for ProcessedFolderRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessedFolderRuntime {
    pub fn new() -> Self {
        Self::with_clock(Arc::new(UtcProcessedFolderClock))
    }

    pub fn with_clock(clock: Arc<dyn ProcessedFolderClock>) -> Self {
        Self {
            in_flight: Arc::new(Mutex::new(HashSet::new())),
            scan_locks: Arc::new(Mutex::new(HashMap::new())),
            clock,
        }
    }

    pub fn now(&self) -> DateTime<Utc> {
        self.clock.now()
    }

    /// Lock the folder, waiting if a scan or another action holds it.
    pub async fn lock_folder(&self, folder_id: &str) -> tokio::sync::OwnedMutexGuard<()> {
        self.scan_mutex(folder_id).await.lock_owned().await
    }

    /// Lock the folder only when no scan holds it.
    pub async fn try_lock_folder(
        &self,
        folder_id: &str,
    ) -> Result<tokio::sync::OwnedMutexGuard<()>, ()> {
        self.scan_mutex(folder_id)
            .await
            .try_lock_owned()
            .map_err(|_| ())
    }

    async fn scan_mutex(&self, folder_id: &str) -> Arc<Mutex<()>> {
        let mut locks = self.scan_locks.lock().await;
        locks
            .entry(folder_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    #[allow(dead_code)] // Core's import task is the caller. The base branch only tests the set.
    pub async fn is_in_flight(&self, entry_id: &str) -> bool {
        self.in_flight.lock().await.contains(entry_id)
    }

    #[allow(dead_code)] // Core's import task is the caller. The base branch only tests the set.
    pub async fn mark_in_flight(&self, entry_id: &str) {
        self.in_flight.lock().await.insert(entry_id.to_string());
    }

    #[allow(dead_code)] // Core's import task is the caller. The base branch only tests the set.
    pub async fn clear_in_flight(&self, entry_id: &str) {
        self.in_flight.lock().await.remove(entry_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processed_folder_clock_defaults_to_utc_and_accepts_a_fixed_instant() {
        let fixed = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let runtime = ProcessedFolderRuntime::with_clock(Arc::new(move || fixed));
        assert_eq!(runtime.now(), fixed);
        let _ = ProcessedFolderRuntime::new().now();
    }

    #[tokio::test]
    async fn processed_folder_scan_lock_is_per_folder() {
        let runtime = ProcessedFolderRuntime::new();
        let held = runtime.try_lock_folder("folder-a").await.unwrap();
        assert!(runtime.try_lock_folder("folder-a").await.is_err());
        assert!(runtime.try_lock_folder("folder-b").await.is_ok());
        drop(held);
        assert!(runtime.try_lock_folder("folder-a").await.is_ok());
        runtime.mark_in_flight("entry-1").await;
        assert!(runtime.is_in_flight("entry-1").await);
        runtime.clear_in_flight("entry-1").await;
        assert!(!runtime.is_in_flight("entry-1").await);
    }
}
