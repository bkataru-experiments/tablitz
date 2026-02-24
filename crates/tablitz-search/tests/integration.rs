use tablitz_core::{Tab, TabGroup, TabSession, SessionSource};
use tablitz_search::{FuzzySearcher, TitleNormalizer, DedupEngine, DedupStrategy};
use chrono::Utc;
use url::Url;

fn make_session() -> TabSession {
    let make_tab = |id: &str, url: &str, title: &str| Tab {
        id: id.to_string(),
        url: Url::parse(url).unwrap(),
        title: title.to_string(),
        favicon_url: None,
        added_at: Utc::now(),
    };
    TabSession {
        version: 1,
        source: SessionSource::Unknown,
        created_at: Utc::now(),
        imported_at: Utc::now(),
        groups: vec![
            TabGroup {
                id: "g1".to_string(),
                label: None,
                created_at: Utc::now(),
                pinned: false, locked: false, starred: false,
                tabs: vec![
                    make_tab("t1", "https://doc.rust-lang.org/book/", "The Rust Programming Language"),
                    make_tab("t2", "https://crates.io", "crates.io: Rust Package Registry"),
                    make_tab("t3", "https://github.com/tokio-rs/tokio", "tokio-rs/tokio: async runtime"),
                ],
            },
        ],
    }
}

#[test]
fn test_fuzzy_search_finds_rust() {
    let session = make_session();
    let results = FuzzySearcher::search("rust", &session);
    assert!(!results.is_empty());
}

#[test]
fn test_fuzzy_search_empty_query() {
    let session = make_session();
    let results = FuzzySearcher::search("", &session);
    let _ = results;
}

#[test]
fn test_title_normalizer() {
    assert_eq!(TitleNormalizer::normalize("Rust - The Book"), "Rust - The Book");
    assert_eq!(TitleNormalizer::normalize("crates.io | GitHub"), "crates.io");
    assert_eq!(TitleNormalizer::normalize("  spaces  "), "spaces");
}

#[test]
fn test_dedup_exact_url_removes_duplicates() {
    let make_tab = |id: &str, url: &str| Tab {
        id: id.to_string(),
        url: Url::parse(url).unwrap(),
        title: "Title".to_string(),
        favicon_url: None,
        added_at: Utc::now(),
    };
    let session = TabSession {
        version: 1,
        source: SessionSource::Unknown,
        created_at: Utc::now(),
        imported_at: Utc::now(),
        groups: vec![
            TabGroup {
                id: "g1".to_string(), label: None, created_at: Utc::now(),
                pinned: false, locked: false, starred: false,
                tabs: vec![
                    make_tab("t1", "https://example.com/"),
                    make_tab("t2", "https://example.com/"),
                    make_tab("t3", "https://different.com/"),
                ],
            },
        ],
    };
    let result = DedupEngine::dedup(&session, DedupStrategy::ExactUrl);
    assert_eq!(result.original_count, 3);
    assert_eq!(result.deduplicated_count, 2);
    assert_eq!(result.removed.len(), 1);
}

#[test]
fn test_dedup_normalized_url() {
    let make_tab = |id: &str, url: &str| Tab {
        id: id.to_string(),
        url: Url::parse(url).unwrap(),
        title: "Title".to_string(),
        favicon_url: None,
        added_at: Utc::now(),
    };
    let session = TabSession {
        version: 1,
        source: SessionSource::Unknown,
        created_at: Utc::now(),
        imported_at: Utc::now(),
        groups: vec![
            TabGroup {
                id: "g1".to_string(), label: None, created_at: Utc::now(),
                pinned: false, locked: false, starred: false,
                tabs: vec![
                    make_tab("t1", "https://example.com/page"),
                    make_tab("t2", "https://example.com/page?utm_source=twitter"),

                    make_tab("t3", "https://different.com/"),
                ],
            },
        ],
    };
    let result = DedupEngine::dedup(&session, DedupStrategy::NormalizedUrl);
    assert_eq!(result.original_count, 3);
    assert_eq!(result.deduplicated_count, 2);
}
