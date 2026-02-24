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

fn make_group(id: &str, label: Option<&str>, tabs: Vec<Tab>) -> TabGroup {
    TabGroup {
        id: id.to_string(),
        label: label.map(str::to_string),
        created_at: Utc::now(),
        pinned: false,
        locked: false,
        starred: false,
        tabs,
    }
}

fn make_test_session() -> TabSession {
    TabSession {
        version: 1,
        source: SessionSource::Unknown,
        created_at: Utc::now(),
        imported_at: Utc::now(),
        groups: vec![
            make_group("test-group-1", Some("Test Group"), vec![
                make_tab("test-tab-1", "https://example.com/rust", "Rust Programming Language"),
                make_tab("test-tab-2", "https://example.com/cargo", "Cargo Package Manager"),
            ]),
        ],
    }
}

async fn open_store() -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let store = Store::open(&db_path).await.unwrap();
    (store, dir)
}

// ─── Basic CRUD ────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_insert_and_retrieve() {
    let (store, _dir) = open_store().await;
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
    let (store, _dir) = open_store().await;
    let session = make_test_session();

    let stats1 = store.insert_session(&session).await.unwrap();
    assert_eq!(stats1.groups_inserted, 1);

    let stats2 = store.insert_session(&session).await.unwrap();
    assert_eq!(stats2.groups_inserted, 0);
    assert_eq!(stats2.groups_skipped, 1);
}

#[tokio::test]
async fn test_search_by_title() {
    let (store, _dir) = open_store().await;
    store.insert_session(&make_test_session()).await.unwrap();

    let results = store.search_by_title("Rust").await.unwrap();
    assert!(!results.is_empty());
    assert!(results.iter().any(|t| t.title.contains("Rust")));
}

#[tokio::test]
async fn test_get_stats() {
    let (store, _dir) = open_store().await;
    store.insert_session(&make_test_session()).await.unwrap();

    let stats = store.get_stats().await.unwrap();
    assert_eq!(stats.total_groups, 1);
    assert_eq!(stats.total_tabs, 2);
}

#[tokio::test]
async fn test_replace_tabs_for_group() {
    let (store, _dir) = open_store().await;
    store.insert_session(&make_test_session()).await.unwrap();

    let groups = store.get_all_groups().await.unwrap();
    let mut group = groups[0].clone();
    group.tabs.truncate(1);
    store.replace_tabs_for_group(&group).await.unwrap();

    let updated = store.get_all_groups().await.unwrap();
    assert_eq!(updated[0].tabs.len(), 1);
}

// ─── Unicode / long-URL handling ───────────────────────────────────────────

/// Real OneTab data shape: unicode in tab titles (⟨ε|Δ⟩, em-dashes, etc.)
#[tokio::test]
async fn test_unicode_titles_stored_and_retrieved() {
    let (store, _dir) = open_store().await;
    let session = TabSession {
        version: 1,
        source: SessionSource::Unknown,
        created_at: Utc::now(),
        imported_at: Utc::now(),
        groups: vec![make_group("unicode-group", None, vec![
            make_tab("u1",
                "https://x.com/baalatejakataru/status/1902846354617143514",
                "#pragma omp ⟨ε|Δ⟩ on X: \"lol i was so heated back then\"",
            ),
            make_tab("u2",
                "https://en.wikipedia.org/wiki/Mellin_transform",
                "Mellin transform — Wikipedia",
            ),
        ])],
    };
    store.insert_session(&session).await.unwrap();

    let groups = store.get_all_groups().await.unwrap();
    assert_eq!(groups[0].tabs[0].title, "#pragma omp ⟨ε|Δ⟩ on X: \"lol i was so heated back then\"");
    assert_eq!(groups[0].tabs[1].title, "Mellin transform — Wikipedia");
}

/// Real OneTab data shape: very long Google Search URLs (1000+ chars)
#[tokio::test]
async fn test_long_urls_stored_correctly() {
    let (store, _dir) = open_store().await;
    let long_url = "https://www.google.com/search?q=how+do+I+use+scp%3F+I+have+a+local+project+%28tabblitz%29&sourceid=chrome&ie=UTF-8&udm=50&aep=48&cud=0&qsubts=1771704930126&source=chrome.crn.obic&mstk=AUtExfA5nmxqURLKiG3A78akoMyCkq1mjMW9SFXJvyoAhCsJK9meUayjPuIwA7lvQwj-Z2ocW5qztW2qpkWWy1nmIk_m6AWZQ7iDFnoSOv0BaOK4Y0stu93vnrIHZHfj0_ZbSPXsagSkhc-Nnp6X73ofNHlSoGXu_NENGnnVRMTftE31pFojNEtvW7uAUdjMbEk5k5ZvIsIrkGg6Q1KRZw7TJVtBUgQJcSuR4ffGj51lGBOvhMxYZNwQKrz3QlgQIwk8-wZCKIDLsAgC5uecIh1U6tBac8D6SckILEE";
    let session = TabSession {
        version: 1,
        source: SessionSource::Unknown,
        created_at: Utc::now(),
        imported_at: Utc::now(),
        groups: vec![make_group("long-url-group", None, vec![
            make_tab("l1", long_url, "how do I use scp? - Google Search"),
        ])],
    };
    store.insert_session(&session).await.unwrap();
    let groups = store.get_all_groups().await.unwrap();
    assert_eq!(groups[0].tabs[0].url.as_str(), long_url);
}

// ─── Multiple groups ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_multiple_groups_inserted_and_retrieved() {
    let (store, _dir) = open_store().await;
    let session = TabSession {
        version: 1,
        source: SessionSource::Unknown,
        created_at: Utc::now(),
        imported_at: Utc::now(),
        groups: vec![
            make_group("g1", Some("Rust"), vec![
                make_tab("t1", "https://doc.rust-lang.org/book/", "The Rust Programming Language"),
                make_tab("t2", "https://crates.io", "crates.io: Rust Package Registry"),
            ]),
            make_group("g2", None, vec![
                make_tab("t3", "https://github.com/tokio-rs/tokio", "tokio-rs/tokio"),
                make_tab("t4", "https://docs.rs/tokio", "tokio - Rust"),
                make_tab("t5", "https://tokio.rs", "Tokio - async Rust"),
            ]),
            make_group("g3", Some("Daily Reading"), vec![
                make_tab("t6", "https://app.daily.dev", "daily.dev"),
            ]),
        ],
    };
    let stats = store.insert_session(&session).await.unwrap();
    assert_eq!(stats.groups_inserted, 3);
    assert_eq!(stats.tabs_inserted, 6);

    let groups = store.get_all_groups().await.unwrap();
    assert_eq!(groups.len(), 3);
    let total: usize = groups.iter().map(|g| g.tabs.len()).sum();
    assert_eq!(total, 6);
}

// ─── Stats / domain counting ───────────────────────────────────────────────

#[tokio::test]
async fn test_stats_top_domains() {
    let (store, _dir) = open_store().await;
    let session = TabSession {
        version: 1,
        source: SessionSource::Unknown,
        created_at: Utc::now(),
        imported_at: Utc::now(),
        groups: vec![make_group("g1", None, vec![
            make_tab("t1", "https://github.com/rust-lang/rust", "Rust"),
            make_tab("t2", "https://github.com/tokio-rs/tokio", "Tokio"),
            make_tab("t3", "https://github.com/serde-rs/serde", "Serde"),
            make_tab("t4", "https://youtube.com/watch?v=1", "YouTube 1"),
            make_tab("t5", "https://youtube.com/watch?v=2", "YouTube 2"),
            make_tab("t6", "https://arxiv.org/abs/1905.11946", "EfficientNet"),
        ])],
    };
    store.insert_session(&session).await.unwrap();

    let stats = store.get_stats().await.unwrap();
    assert_eq!(stats.total_tabs, 6);
    assert!(!stats.top_domains.is_empty());
    // github.com should appear first (3 tabs)
    let github_entry = stats.top_domains.iter().find(|(d, _)| d == "github.com");
    assert!(github_entry.is_some(), "github.com should be in top domains");
    assert_eq!(github_entry.unwrap().1, 3);
}

#[tokio::test]
async fn test_stats_date_range() {
    let (store, _dir) = open_store().await;
    store.insert_session(&make_test_session()).await.unwrap();

    let stats = store.get_stats().await.unwrap();
    assert!(stats.oldest_group.is_some());
    assert!(stats.newest_group.is_some());
}

// ─── Search ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_search_by_title_case_insensitive() {
    let (store, _dir) = open_store().await;
    store.insert_session(&make_test_session()).await.unwrap();

    let lower = store.search_by_title("rust").await.unwrap();
    let upper = store.search_by_title("RUST").await.unwrap();
    assert!(!lower.is_empty());
    assert!(!upper.is_empty());
}

#[tokio::test]
async fn test_search_no_results() {
    let (store, _dir) = open_store().await;
    store.insert_session(&make_test_session()).await.unwrap();

    let results = store.search_by_title("zzznonexistentzzzz").await.unwrap();
    assert!(results.is_empty());
}

// ─── get_session round-trip ────────────────────────────────────────────────

#[tokio::test]
async fn test_get_session_round_trip() {
    let (store, _dir) = open_store().await;
    let session = make_test_session();
    store.insert_session(&session).await.unwrap();

    let retrieved = store.get_session().await.unwrap();
    assert_eq!(retrieved.groups.len(), 1);
    assert_eq!(retrieved.groups[0].id, "test-group-1");
    assert_eq!(retrieved.total_tab_count(), 2);
}

// ─── Large session (stress test) ───────────────────────────────────────────

#[tokio::test]
async fn test_large_session_insert() {
    let (store, _dir) = open_store().await;
    // 50 groups × 20 tabs = 1000 tabs (representative of real 20k-tab collections)
    let groups: Vec<TabGroup> = (0..50).map(|gi| {
        let tabs = (0..20).map(|ti| make_tab(
            &format!("t-{}-{}", gi, ti),
            &format!("https://example-{}.com/page-{}", gi, ti),
            &format!("Tab {} in Group {}", ti, gi),
        )).collect();
        make_group(&format!("g-{}", gi), None, tabs)
    }).collect();

    let session = TabSession {
        version: 1,
        source: SessionSource::Unknown,
        created_at: Utc::now(),
        imported_at: Utc::now(),
        groups,
    };
    let stats = store.insert_session(&session).await.unwrap();
    assert_eq!(stats.groups_inserted, 50);
    assert_eq!(stats.tabs_inserted, 1000);

    let retrieved = store.get_all_groups().await.unwrap();
    assert_eq!(retrieved.len(), 50);
    let total: usize = retrieved.iter().map(|g| g.tabs.len()).sum();
    assert_eq!(total, 1000);
}

// ─── Live test (skipped unless TABLITZ_LIVE_DB set) ─────────────────────────

#[tokio::test]
async fn test_live_store_stats_if_available() {
    let db_path = match std::env::var("TABLITZ_LIVE_DB") {
        Ok(p) => std::path::PathBuf::from(p),
        Err(_) => return,
    };
    let store = Store::open(&db_path).await.expect("failed to open live store");
    let stats = store.get_stats().await.expect("failed to get stats");
    eprintln!("live store: {} groups, {} tabs", stats.total_groups, stats.total_tabs);
    assert!(stats.total_groups > 0);
    assert!(stats.total_tabs > 0);
}
