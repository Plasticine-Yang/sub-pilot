//! Recent-projects index (ADR-0003: a lightweight index in the app data dir,
//! separate from the Project directories themselves).
//!
//! The list operations (`upsert`, `reconcile`) are pure so they are unit
//! tested directly; loading/saving the index file and probing the filesystem
//! live in thin wrappers exercised end-to-end.

use crate::project::ProjectStatus;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

/// One entry in the recent-projects list. Enough for the home screen to render
/// a row and reopen the project; the authoritative state lives in `project.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentEntry {
    pub id: String,
    pub video_file_name: String,
    pub status: ProjectStatus,
    /// Monotonic recency key (e.g. ms since epoch); higher is more recent.
    pub updated_at: u64,
}

/// The persisted recent-projects list, most-recent-first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RecentIndex {
    pub entries: Vec<RecentEntry>,
}

impl RecentIndex {
    /// Inserts or updates `entry`, keeping the list deduplicated by `id` and
    /// ordered most-recent-first. An existing entry for the same id is replaced
    /// and moved to the front.
    pub fn upsert(&mut self, entry: RecentEntry) {
        self.entries.retain(|e| e.id != entry.id);
        self.entries.insert(0, entry);
    }

    /// Drops entries whose project id is not in `existing` — i.e. the project
    /// directory was deleted or moved out from under the index (graceful
    /// handling of an index/disk mismatch).
    pub fn reconcile(&mut self, existing: &HashSet<String>) {
        self.entries.retain(|e| existing.contains(&e.id));
    }
}

/// Canonical file name of the recent-projects index in the app data dir.
pub const INDEX_FILE: &str = "recent.json";

/// Loads the index from `app_data_dir`, returning an empty index when the file
/// is absent or unreadable (a missing index is not an error — it's a fresh app).
pub fn load(app_data_dir: &Path) -> RecentIndex {
    match std::fs::read_to_string(app_data_dir.join(INDEX_FILE)) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => RecentIndex::default(),
    }
}

/// Writes the index into `app_data_dir` (pretty-printed).
pub fn save(app_data_dir: &Path, index: &RecentIndex) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(index).map_err(std::io::Error::from)?;
    std::fs::write(app_data_dir.join(INDEX_FILE), json)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, updated_at: u64) -> RecentEntry {
        RecentEntry {
            id: id.to_string(),
            video_file_name: format!("{id}.mkv"),
            status: ProjectStatus::Transcribed,
            updated_at,
        }
    }

    #[test]
    fn upsert_prepends_new_entries_most_recent_first() {
        let mut index = RecentIndex::default();
        index.upsert(entry("a", 1));
        index.upsert(entry("b", 2));
        let ids: Vec<_> = index.entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["b", "a"]);
    }

    #[test]
    fn upsert_replaces_and_promotes_existing_id() {
        let mut index = RecentIndex::default();
        index.upsert(entry("a", 1));
        index.upsert(entry("b", 2));

        let mut updated = entry("a", 3);
        updated.status = ProjectStatus::Exported;
        index.upsert(updated);

        let ids: Vec<_> = index.entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["a", "b"], "updated entry moves to front, no dupes");
        assert_eq!(index.entries[0].status, ProjectStatus::Exported);
    }

    #[test]
    fn reconcile_drops_missing_projects() {
        let mut index = RecentIndex::default();
        index.upsert(entry("a", 1));
        index.upsert(entry("b", 2));
        index.upsert(entry("c", 3));

        let existing: HashSet<String> = ["a".to_string(), "c".to_string()].into_iter().collect();
        index.reconcile(&existing);

        let ids: Vec<_> = index.entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["c", "a"], "b was removed, order preserved");
    }

    #[test]
    fn reconcile_to_empty_when_nothing_exists() {
        let mut index = RecentIndex::default();
        index.upsert(entry("a", 1));
        index.reconcile(&HashSet::new());
        assert!(index.entries.is_empty());
    }

    #[test]
    fn load_missing_index_is_empty_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()), RecentIndex::default());
    }

    #[test]
    fn load_ignores_a_corrupt_index_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(INDEX_FILE), "not json {{").unwrap();
        assert_eq!(load(dir.path()), RecentIndex::default());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = RecentIndex::default();
        index.upsert(entry("a", 1));
        index.upsert(entry("b", 2));

        save(dir.path(), &index).unwrap();
        assert_eq!(load(dir.path()), index);
    }
}
