use tablitz_sync::SyncManager;
use tablitz_store::Store;
use tablitz_core::{Tab, TabGroup, TabSession, SessionSource};
use chrono::Utc;
use url::Url;

// ─── Helpers ───────────────────────────────────────────────────────────────

fn make_tab(id: &str, url: &str, title: &str) -> Tab {
    Tab {
        id: id.to_string(),
        url: Url::parse(url).unwrap(),
        title: title.to_string(),
        favicon_url: None,
        added_at: Utc::now(),
    }
}

fn make_session(extra_suffix: &str) -> TabSession {
    TabSession {
        version: 1,
        source: SessionSource::Unknown,
        created_at: Utc::now(),
        imported_at: Utc::now(),
        groups: vec![TabGroup {
            id: format!("sync-group-{}", extra_suffix),
            label: Some(format!("Sync Test {}", extra_suffix)),
            created_at: Utc::now(),
            pinned: false,
            locked: false,
            starred: false,
            tabs: vec![
                make_tab(
                    &format!("tab-1-{}", extra_suffix),
                    "https://doc.rust-lang.org/book/",
                    "The Rust Programming Language",
                ),
                make_tab(
                    &format!("tab-2-{}", extra_suffix),
                    "https://crates.io",
                    "crates.io: Rust Package Registry",
                ),
            ],
        }],
    }
}

/// Set up a temporary store pre-populated with test data.
async fn setup_store(dir: &tempfile::TempDir, suffix: &str) -> Store {
    let db_path = dir.path().join("test.db");
    let store = Store::open(&db_path).await.unwrap();
    store.insert_session(&make_session(suffix)).await.unwrap();
    store
}

/// Configure git identity for the temp repo (needed in CI/testing environments).
fn git_config(repo_path: &std::path::Path) {
    for (k, v) in [
        ("user.email", "test@tablitz.test"),
        ("user.name", "Tablitz Test"),
        ("commit.gpgsign", "false"),
    ] {
        std::process::Command::new("git")
            .args(["config", "--local", k, v])
            .current_dir(repo_path)
            .output()
            .ok();
    }
}

// ─── Snapshot ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_snapshot_creates_commit() {
    let store_dir = tempfile::tempdir().unwrap();
    let repo_dir = tempfile::tempdir().unwrap();

    let store = setup_store(&store_dir, "snap1").await;
    let sync = SyncManager::new(repo_dir.path());

    sync.init_repo().unwrap();
    git_config(repo_dir.path());

    let hash = sync.snapshot(&store).await.unwrap();
    assert!(!hash.is_empty(), "snapshot should return a commit hash");
    assert_ne!(hash, "unknown", "hash should not be 'unknown'");

    // The snapshot file should exist on disk
    assert!(sync.snapshot_path().exists(), "snapshot file should exist");
}

#[tokio::test]
async fn test_snapshot_contains_valid_json() {
    let store_dir = tempfile::tempdir().unwrap();
    let repo_dir = tempfile::tempdir().unwrap();

    let store = setup_store(&store_dir, "snap-json").await;
    let sync = SyncManager::new(repo_dir.path());
    sync.init_repo().unwrap();
    git_config(repo_dir.path());

    sync.snapshot(&store).await.unwrap();

    let json = std::fs::read_to_string(sync.snapshot_path()).unwrap();
    let _: tablitz_core::TabSession = serde_json::from_str(&json)
        .expect("snapshot file should contain valid JSON matching TabSession");
}

// ─── List snapshots ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_list_snapshots_returns_entries() {
    let store_dir = tempfile::tempdir().unwrap();
    let repo_dir = tempfile::tempdir().unwrap();

    let store = setup_store(&store_dir, "list1").await;
    let sync = SyncManager::new(repo_dir.path());
    sync.init_repo().unwrap();
    git_config(repo_dir.path());

    sync.snapshot(&store).await.unwrap();

    let entries = sync.list_snapshots(10).unwrap();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].message.contains("tablitz snapshot"));
    assert!(!entries[0].hash.is_empty());
}

#[tokio::test]
async fn test_list_snapshots_multiple_entries() {
    let store_dir = tempfile::tempdir().unwrap();
    let repo_dir = tempfile::tempdir().unwrap();

    let store = setup_store(&store_dir, "multi").await;
    let sync = SyncManager::new(repo_dir.path());
    sync.init_repo().unwrap();
    git_config(repo_dir.path());

    // Take two snapshots
    let h1 = sync.snapshot(&store).await.unwrap();

    // Add more data and snapshot again
    let session2 = make_session("multi-2");
    store.insert_session(&session2).await.unwrap();
    let h2 = sync.snapshot(&store).await.unwrap();

    assert_ne!(h1, h2, "two different snapshots should have different hashes");

    let entries = sync.list_snapshots(10).unwrap();
    assert_eq!(entries.len(), 2, "should list both snapshots");
}

#[tokio::test]
async fn test_list_snapshots_limit_respected() {
    let store_dir = tempfile::tempdir().unwrap();
    let repo_dir = tempfile::tempdir().unwrap();

    let store = setup_store(&store_dir, "limit").await;
    let sync = SyncManager::new(repo_dir.path());
    sync.init_repo().unwrap();
    git_config(repo_dir.path());

    // Take 3 snapshots
    for i in 0..3 {
        let s = make_session(&format!("limit-{}", i));
        store.insert_session(&s).await.unwrap();
        sync.snapshot(&store).await.unwrap();
    }

    let entries = sync.list_snapshots(2).unwrap();
    assert_eq!(entries.len(), 2, "limit should be respected");
}

// ─── Restore ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_restore_imports_data() {
    let source_store_dir = tempfile::tempdir().unwrap();
    let target_store_dir = tempfile::tempdir().unwrap();
    let repo_dir = tempfile::tempdir().unwrap();

    // Snapshot from source store
    let source = setup_store(&source_store_dir, "restore-src").await;
    let sync = SyncManager::new(repo_dir.path());
    sync.init_repo().unwrap();
    git_config(repo_dir.path());
    sync.snapshot(&source).await.unwrap();

    // Restore into empty target store
    let target = Store::open(&target_store_dir.path().join("target.db")).await.unwrap();
    let (groups_inserted, tabs_inserted) = sync.restore(&target).await.unwrap();
    assert_eq!(groups_inserted, 1);
    assert_eq!(tabs_inserted, 2);

    let groups = target.get_all_groups().await.unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].tabs.len(), 2);
}

#[tokio::test]
async fn test_restore_is_idempotent() {
    let store_dir = tempfile::tempdir().unwrap();
    let repo_dir = tempfile::tempdir().unwrap();

    let store = setup_store(&store_dir, "idempotent").await;
    let sync = SyncManager::new(repo_dir.path());
    sync.init_repo().unwrap();
    git_config(repo_dir.path());
    sync.snapshot(&store).await.unwrap();

    // First restore: data already in store, so 0 inserted
    let (inserted, _) = sync.restore(&store).await.unwrap();
    assert_eq!(inserted, 0, "idempotent restore of existing data should insert 0 groups");
}

// ─── Restore from commit ───────────────────────────────────────────────────

#[tokio::test]
async fn test_restore_from_specific_commit() {
    let source_dir = tempfile::tempdir().unwrap();
    let target_dir = tempfile::tempdir().unwrap();
    let repo_dir = tempfile::tempdir().unwrap();

    let source = setup_store(&source_dir, "commit-src").await;
    let sync = SyncManager::new(repo_dir.path());
    sync.init_repo().unwrap();
    git_config(repo_dir.path());

    // First snapshot: 1 group
    let h1 = sync.snapshot(&source).await.unwrap();

    // Add second group and take another snapshot
    let session2 = make_session("commit-v2");
    source.insert_session(&session2).await.unwrap();
    let _h2 = sync.snapshot(&source).await.unwrap();

    // Restore into empty target from the FIRST commit (only 1 group)
    let target = Store::open(&target_dir.path().join("target.db")).await.unwrap();
    let (inserted, _) = sync.restore_from_commit(&target, &h1).await.unwrap();
    assert_eq!(inserted, 1, "restoring from first commit should import 1 group");

    let groups = target.get_all_groups().await.unwrap();
    assert_eq!(groups.len(), 1);
}

// ─── Live test ────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_live_snapshot_restore_roundtrip() {
    let db_path = match std::env::var("TABLITZ_LIVE_DB") {
        Ok(p) => std::path::PathBuf::from(p),
        Err(_) => return,
    };
    let repo_dir = tempfile::tempdir().unwrap();

    let store = Store::open(&db_path).await.expect("open live store");
    let pre_stats = store.get_stats().await.unwrap();

    let sync = SyncManager::new(repo_dir.path());
    sync.init_repo().unwrap();
    git_config(repo_dir.path());

    let hash = sync.snapshot(&store).await.expect("snapshot live store");
    eprintln!("live snapshot hash: {}", hash);
    assert!(!hash.is_empty());

    // Restore into the same store: should be idempotent
    let (inserted, _) = sync.restore(&store).await.expect("restore");
    assert_eq!(inserted, 0, "restoring into same store should insert 0 (idempotent)");

    let post_stats = store.get_stats().await.unwrap();
    assert_eq!(pre_stats.total_groups, post_stats.total_groups);
}
