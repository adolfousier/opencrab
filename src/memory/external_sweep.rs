//! Tier-2 external sweep (#1051 / ADR-002).
//!
//! A periodic incremental walk over the configured external roots that
//! discovers additions, prunes deletions, and reconciles config changes —
//! without a filesystem watcher. Directory mtimes are the pruning signal:
//! adding or removing an entry bumps the parent directory's mtime, so a
//! subtree whose mtime is unchanged keeps its prior file list without a
//! single file read. Modified-file content that does not change the dir
//! mtime is tier-1's job at search time, not the sweep's.

use super::db::Store;
use super::external::{ResolvedRoot, excluded, resolve_roots};
use super::{COLLECTION_EXTERNAL, external_excludes};
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Mutex as StdMutex;
use std::time::SystemTime;

/// Process-local sweep state. `None` = cold: the next sweep treats every
/// directory as changed (a full pass), which is exactly boot behaviour.
struct SweepState {
    dir_mtimes: HashMap<PathBuf, SystemTime>,
}

static SWEEP_STATE: StdMutex<Option<SweepState>> = StdMutex::new(None);

/// Outcome of one sweep pass.
#[derive(Debug, Default)]
pub(crate) struct SweepReport {
    /// Documents newly indexed (content hash changed or new).
    pub indexed: usize,
    /// Documents deactivated (deleted on disk or config path removed).
    pub pruned: usize,
    /// Matching files currently on disk across all roots.
    pub on_disk: usize,
    /// Files actually read this pass (only under changed dirs).
    pub reads: usize,
}

impl SweepReport {
    pub fn log(&self) {
        tracing::info!(
            "Memory external sweep: indexed={} pruned={} on_disk={} reads={}",
            self.indexed,
            self.pruned,
            self.on_disk,
            self.reads
        );
    }
}

/// Incremental sweep over one root. Walks every directory (metadata only),
/// reads files ONLY under directories whose mtime moved, records on-disk
/// keys, and indexes changed files (FTS-only; embeddings ride the backfill).
#[allow(clippy::too_many_arguments)]
fn sweep_root(
    store: &Store,
    root: &ResolvedRoot,
    excludes: &[glob::Pattern],
    prior_dirs: &HashMap<PathBuf, SystemTime>,
    new_dirs: &mut HashMap<PathBuf, SystemTime>,
    on_disk: &mut BTreeSet<String>,
    report: &mut SweepReport,
) {
    let mut stack = vec![root.root.clone()];
    while let Some(dir) = stack.pop() {
        let mtime = std::fs::metadata(&dir).and_then(|m| m.modified()).ok();
        if let Some(t) = mtime {
            new_dirs.insert(dir.clone(), t);
        }
        // A dir is "changed" when its mtime moved or it is new. Unchanged
        // dirs keep their prior file list with zero file reads.
        let dir_changed = match (mtime, prior_dirs.get(&dir)) {
            (Some(now), Some(prev)) => now > *prev,
            (Some(_), None) => true,
            (None, _) => false,
        };
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_symlink() {
                continue;
            }
            let Ok(rel) = path.strip_prefix(&root.root) else {
                continue;
            };
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if !excluded(&rel_str, &name, true, excludes) {
                    stack.push(path);
                }
            } else if path.is_file()
                && !excluded(&rel_str, &name, false, excludes)
                && root.pattern.matches(&rel_str)
            {
                let key = path.to_string_lossy().to_string();
                on_disk.insert(key.clone());
                if dir_changed {
                    report.reads += 1;
                    match std::fs::read_to_string(&path) {
                        Ok(body) if !body.trim().is_empty() => {
                            match super::index::index_file_sync_keyed(
                                store,
                                COLLECTION_EXTERNAL,
                                &key,
                                &body,
                            ) {
                                Ok(true) => report.indexed += 1,
                                Ok(false) => {}
                                Err(e) => {
                                    tracing::warn!("memory sweep: index failed {key}: {e}")
                                }
                            }
                        }
                        Ok(_) => {
                            on_disk.remove(&key);
                        }
                        Err(e) => {
                            tracing::warn!("memory sweep: unreadable {key}: {e}");
                            on_disk.remove(&key);
                        }
                    }
                }
            }
        }
    }
}

/// Pure, testable sweep core. Runs over the given roots with an explicit
/// prior-dir-mtime snapshot (`None` = cold/full pass) and returns the report
/// plus the fresh dir-mtime snapshot for the next pass. Touches no global
/// state, so tests run concurrently without clobbering each other.
pub(crate) fn sweep_external_with(
    store: &Store,
    roots: &[ResolvedRoot],
    excludes: &[glob::Pattern],
    prior_dirs: Option<&HashMap<PathBuf, SystemTime>>,
) -> (SweepReport, HashMap<PathBuf, SystemTime>) {
    let empty = HashMap::new();
    let prior = prior_dirs.unwrap_or(&empty);
    let mut new_dirs = HashMap::new();
    let mut on_disk = BTreeSet::new();
    let mut report = SweepReport::default();

    for root in roots {
        sweep_root(
            store,
            root,
            excludes,
            prior,
            &mut new_dirs,
            &mut on_disk,
            &mut report,
        );
    }

    // Prune anything indexed that is no longer on disk under a configured
    // root: deleted files, removed config paths, repointed root symlinks.
    if let Ok(db_paths) = store.get_active_document_paths(COLLECTION_EXTERNAL) {
        for db_path in &db_paths {
            if !on_disk.contains(db_path) {
                let _ = store.deactivate_document(COLLECTION_EXTERNAL, db_path);
                report.pruned += 1;
                tracing::debug!("memory sweep: pruned external document {db_path}");
            }
        }
    }

    report.on_disk = on_disk.len();
    (report, new_dirs)
}

/// Config-driven sweep: resolves roots fresh from `[memory].extra_paths`
/// (live-read, so config changes reconcile within one interval — Q15),
/// manages the process-local dir-mtime state, and returns the report.
pub(crate) fn sweep_external(store: &Store) -> SweepReport {
    let (roots, resolve_report) = resolve_roots();
    resolve_report.log();
    let excludes: Vec<glob::Pattern> = external_excludes()
        .iter()
        .filter_map(|s| glob::Pattern::new(s).ok())
        .collect();

    let prior = SWEEP_STATE.lock().unwrap_or_else(|e| e.into_inner()).take();
    let prior_dirs = prior.map(|s| s.dir_mtimes);

    let (report, new_dirs) = sweep_external_with(store, &roots, &excludes, prior_dirs.as_ref());

    {
        let mut g = SWEEP_STATE.lock().unwrap_or_else(|e| e.into_inner());
        *g = Some(SweepState {
            dir_mtimes: new_dirs,
        });
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use glob::Pattern;
    use std::path::Path;

    fn md_root(path: &Path) -> ResolvedRoot {
        ResolvedRoot {
            root: path.to_path_buf(),
            pattern: Pattern::new("**/*.md").unwrap(),
        }
    }

    #[test]
    fn cold_sweep_indexes_then_warm_sweep_reads_nothing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().to_path_buf();
        std::fs::write(root.join("a.md"), "# a").unwrap();
        let store = Store::open(&tmp.path().join("mem.db")).expect("store");

        let roots = vec![md_root(&root)];
        let excludes = vec![];

        let (r1, dirs1) = sweep_external_with(&store, &roots, &excludes, None);
        assert_eq!(r1.indexed, 1, "cold pass indexes the one file");
        assert_eq!(r1.on_disk, 1);
        assert_eq!(r1.pruned, 0);

        let (r2, _dirs2) = sweep_external_with(&store, &roots, &excludes, Some(&dirs1));
        assert_eq!(r2.reads, 0, "quiet tree reads nothing");
        assert_eq!(r2.indexed, 0, "quiet tree indexes nothing");
        assert_eq!(r2.pruned, 0);
        assert_eq!(r2.on_disk, 1);
    }

    #[test]
    fn warm_sweep_discovers_additions_and_prunes_deletions() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().to_path_buf();
        std::fs::write(root.join("a.md"), "# a").unwrap();
        let store = Store::open(&tmp.path().join("mem.db")).expect("store");

        let roots = vec![md_root(&root)];
        let excludes = vec![];

        let (_r1, dirs1) = sweep_external_with(&store, &roots, &excludes, None);

        // Add b.md and delete a.md — both bump the root dir's mtime.
        std::fs::write(root.join("b.md"), "# b").unwrap();
        std::fs::remove_file(root.join("a.md")).unwrap();

        let (r2, _dirs2) = sweep_external_with(&store, &roots, &excludes, Some(&dirs1));
        assert_eq!(r2.indexed, 1, "b.md discovered and indexed");
        assert_eq!(r2.pruned, 1, "deleted a.md pruned");
        assert_eq!(r2.on_disk, 1);
    }

    #[test]
    fn removing_a_config_path_prunes_its_documents() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root_a = tmp.path().join("a");
        let root_b = tmp.path().join("b");
        std::fs::create_dir_all(&root_a).unwrap();
        std::fs::create_dir_all(&root_b).unwrap();
        std::fs::write(root_a.join("x.md"), "# x").unwrap();
        std::fs::write(root_b.join("y.md"), "# y").unwrap();
        let store = Store::open(&tmp.path().join("mem.db")).expect("store");

        let excludes = vec![];
        let both = vec![md_root(&root_a), md_root(&root_b)];

        let (r1, dirs1) = sweep_external_with(&store, &both, &excludes, None);
        assert_eq!(r1.indexed, 2, "both roots indexed cold");

        // Config now only lists root_a: root_b's doc must be pruned.
        let only_a = vec![md_root(&root_a)];
        let (r2, _dirs2) = sweep_external_with(&store, &only_a, &excludes, Some(&dirs1));
        assert_eq!(r2.pruned, 1, "root_b doc pruned after config removal");
        assert_eq!(r2.on_disk, 1);
    }

    #[test]
    fn excludes_are_respected_by_the_sweep() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join("node_modules")).unwrap();
        std::fs::write(root.join("keep.md"), "# keep").unwrap();
        std::fs::write(root.join("node_modules/skip.md"), "# skip").unwrap();
        let store = Store::open(&tmp.path().join("mem.db")).expect("store");

        let roots = vec![md_root(&root)];
        let excludes = vec![Pattern::new("node_modules").unwrap()];

        let (r1, _dirs) = sweep_external_with(&store, &roots, &excludes, None);
        assert_eq!(r1.indexed, 1, "only keep.md indexed");
        assert_eq!(r1.on_disk, 1);
    }
}
