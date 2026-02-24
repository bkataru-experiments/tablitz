use tablitz_store::Store;
use tablitz_core::{Tab, TabGroup, TabSession, SessionSource};
use chrono::Utc;
use url::Url;

fn make_test_session() -> TabSession {
    TabSession {
        version: 1,
        source: SessionSource::Unknown,
        created_at: Utc::now(),
        imported_at: Utc::now(),
        groups: vec![
            TabGroup {
                id: "test-group-1".to_string(),
                label: Some("Test Group".to_string()),
                created_at: Utc::now(),
                pinned: false,
                locked: false,
                starred: false,
                tabs: vec![
                    Tab {
                        id: "test-tab-1".to_string(),
                        url: Url::parse("https://example.com/rust").unwrap(),
                        title: "Rust Programming Language".to_string(),
                        favicon_url: None,
                        added_at: Utc::now(),
                    },
                    Tab {
                        id: "test-tab-2".to_string(),
                        url: Url::parse("https://example.com/cargo").unwrap(),
                        title: "Cargo Package Manager".to_string(),
                        favicon_url: None,
                        added_at: Utc::now(),
                    },
                ],
            },
        ],
    }
}

#[tokio::test]
async fn test_insert_and_retrieve() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let store = Store::open(&db_path).await.unwrap();
    let session = make_test_session();
    let stats = store.insert_session(&session).await.unwrap();
    assert_eq!(stats.groups_inserted, 1);
    assert_eq!(stats.tabs_inserted, 2);

    let groups = store.get_all_groups().await.unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].id, "test-group-1");
    assert_eq!(groups[0].tabs.len(), 2);
}

#[tokio::test]
async fn test_idempotent_insert() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let store = Store::open(&db_path).await.unwrap();
    let session = make_test_session();

    let stats1 = store.insert_session(&session).await.unwrap();
    assert_eq!(stats1.groups_inserted, 1);

    let stats2 = store.insert_session(&session).await.unwrap();
    assert_eq!(stats2.groups_inserted, 0);
    assert_eq!(stats2.groups_skipped, 1);
}

#[tokio::test]
async fn test_search_by_title() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let store = Store::open(&db_path).await.unwrap();
    store.insert_session(&make_test_session()).await.unwrap();

    let results = store.search_by_title("Rust").await.unwrap();
    assert!(!results.is_empty());
    assert!(results.iter().any(|t| t.title.contains("Rust")));
}

#[tokio::test]
async fn test_get_stats() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let store = Store::open(&db_path).await.unwrap();
    store.insert_session(&make_test_session()).await.unwrap();

    let stats = store.get_stats().await.unwrap();
    assert_eq!(stats.total_groups, 1);
    assert_eq!(stats.total_tabs, 2);
}

#[tokio::test]
async fn test_replace_tabs_for_group() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let store = Store::open(&db_path).await.unwrap();
    store.insert_session(&make_test_session()).await.unwrap();

    let groups = store.get_all_groups().await.unwrap();
    let mut group = groups[0].clone();
    group.tabs.truncate(1);
    store.replace_tabs_for_group(&group).await.unwrap();

    let updated_groups = store.get_all_groups().await.unwrap();
    assert_eq!(updated_groups[0].tabs.len(), 1);
}
