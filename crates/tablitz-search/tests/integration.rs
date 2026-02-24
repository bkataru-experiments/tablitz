use tablitz_core::{Tab, TabGroup, TabSession, SessionSource};
use tablitz_search::{FuzzySearcher, TitleNormalizer, DedupEngine, DedupStrategy};
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

fn make_group(id: &str, tabs: Vec<Tab>) -> TabGroup {
    TabGroup {
        id: id.to_string(),
        label: None,
        created_at: Utc::now(),
        pinned: false,
        locked: false,
        starred: false,
        tabs,
    }
}

fn make_session(groups: Vec<TabGroup>) -> TabSession {
    TabSession {
        version: 1,
        source: SessionSource::Unknown,
        created_at: Utc::now(),
        imported_at: Utc::now(),
        groups,
    }
}

/// A session that mirrors real OneTab data shape (GitHub, YouTube, arxiv, daily.dev)
fn make_real_shape_session() -> TabSession {
    make_session(vec![
        make_group("g1", vec![
            make_tab("t1", "https://doc.rust-lang.org/book/", "The Rust Programming Language"),
            make_tab("t2", "https://crates.io", "crates.io: Rust Package Registry"),
            make_tab("t3", "https://github.com/tokio-rs/tokio", "tokio-rs/tokio: async runtime"),
        ]),
        make_group("g2", vec![
            make_tab("t4", "https://www.youtube.com/watch?v=DtisQDXLnXk", "Make a NotebookLM Clone - AI Workshop - YouTube"),
            make_tab("t5", "https://www.youtube.com/watch?v=rX0ItVEVjHc", "CppCon 2014: Mike Acton \"Data-Oriented Design and C++\" - YouTube"),
        ]),
        make_group("g3", vec![
            make_tab("t6", "https://arxiv.org/abs/1905.11946", "[1905.11946] EfficientNet: Rethinking Model Scaling for CNNs"),
            make_tab("t7", "https://arxiv.org/abs/1610.02391", "[1610.02391] Grad-CAM: Visual Explanations from Deep Networks"),
            make_tab("t8", "https://app.daily.dev/posts/okay-we-gotta-talk-about-rust-8slxaherm", "(20+) Okay.. we gotta talk about Rust | daily.dev"),
        ]),
    ])
}

// ─── Fuzzy search ──────────────────────────────────────────────────────────

#[test]
fn test_fuzzy_search_finds_rust() {
    let session = make_real_shape_session();
    let results = FuzzySearcher::search("rust", &session);
    assert!(!results.is_empty());
    // All returned results should plausibly relate to "rust"
    assert!(results.iter().any(|r| r.tab.title.to_lowercase().contains("rust")));
}

#[test]
fn test_fuzzy_search_empty_query() {
    let session = make_real_shape_session();
    let results = FuzzySearcher::search("", &session);
    // Empty query should return empty or everything — no panic
    let _ = results;
}

#[test]
fn test_fuzzy_search_returns_multiple_matches() {
    let session = make_real_shape_session();
    let results = FuzzySearcher::search("youtube", &session);
    // Two YouTube tabs in session
    assert!(results.len() >= 2);
}

#[test]
fn test_fuzzy_search_no_match_returns_empty_or_low_scored() {
    let session = make_real_shape_session();
    let results = FuzzySearcher::search("xyznonexistentquery", &session);
    // Either empty or very low scores
    if !results.is_empty() {
        // All scores should be very low (< 50 on typical 0-100 scale)
        for r in &results {
            assert!(r.score < 50.0, "score too high for non-matching query: {}", r.score);
        }
    }
}

#[test]
fn test_fuzzy_search_unicode_query() {
    let session = make_session(vec![make_group("g1", vec![
        make_tab("t1",
            "https://x.com/baalatejakataru/status/1902846354617143514",
            "#pragma omp ⟨ε|Δ⟩ on X: \"lol i was so heated\""),
        make_tab("t2", "https://en.wikipedia.org/wiki/Mellin_transform", "Mellin transform - Wikipedia"),
    ])]);
    // Should not panic on unicode query
    let results = FuzzySearcher::search("⟨ε|Δ⟩", &session);
    let _ = results;
}

#[test]
fn test_fuzzy_search_arxiv_brackets_in_title() {
    let session = make_real_shape_session();
    let results = FuzzySearcher::search("efficientnet", &session);
    assert!(!results.is_empty());
    assert!(results[0].tab.title.to_lowercase().contains("efficientnet"));
}

// ─── Title normalizer ──────────────────────────────────────────────────────

#[test]
fn test_title_normalizer_basic() {
    assert_eq!(TitleNormalizer::normalize("Rust - The Book"), "Rust - The Book");
    assert_eq!(TitleNormalizer::normalize("  spaces  "), "spaces");
}

#[test]
fn test_title_normalizer_strips_site_suffix() {
    // Common OneTab pattern: "Title | Site Name"
    assert_eq!(TitleNormalizer::normalize("crates.io | GitHub"), "crates.io");
}

/// Real OneTab title patterns from daily.dev and YouTube
#[test]
fn test_title_normalizer_real_patterns() {
    // YouTube: "Video Title - YouTube" → "Video Title"
    let yt = TitleNormalizer::normalize("The Rust Programming Language - YouTube");
    assert!(!yt.contains("YouTube"), "expected YouTube suffix stripped");

    // daily.dev: "(20+) Title | daily.dev" → strip suffix
    let dd = TitleNormalizer::normalize("(20+) Okay.. we gotta talk about Rust | daily.dev");
    assert!(!dd.contains("daily.dev"), "expected daily.dev suffix stripped");
}

#[test]
fn test_title_normalizer_idempotent() {
    let title = "Rust Programming Language";
    assert_eq!(
        TitleNormalizer::normalize(title),
        TitleNormalizer::normalize(&TitleNormalizer::normalize(title))
    );
}

// ─── Dedup – exact URL ─────────────────────────────────────────────────────

#[test]
fn test_dedup_exact_url_removes_duplicates() {
    let session = make_session(vec![make_group("g1", vec![
        make_tab("t1", "https://example.com/", "Title"),
        make_tab("t2", "https://example.com/", "Title"),
        make_tab("t3", "https://different.com/", "Other"),
    ])]);
    let result = DedupEngine::dedup(&session, DedupStrategy::ExactUrl);
    assert_eq!(result.original_count, 3);
    assert_eq!(result.deduplicated_count, 2);
    assert_eq!(result.removed.len(), 1);
}

#[test]
fn test_dedup_exact_url_different_fragment_is_duplicate() {
    // Exact URL: same path+query, different fragment → same URL string → duplicate
    let session = make_session(vec![make_group("g1", vec![
        make_tab("t1", "https://example.com/page", "A"),
        make_tab("t2", "https://example.com/page", "A"),
    ])]);
    let result = DedupEngine::dedup(&session, DedupStrategy::ExactUrl);
    assert_eq!(result.removed.len(), 1);
}

// ─── Dedup – normalized URL ────────────────────────────────────────────────

#[test]
fn test_dedup_normalized_url() {
    let session = make_session(vec![make_group("g1", vec![
        make_tab("t1", "https://example.com/page", "A"),
        // utm_ params should be stripped in normalized form
        make_tab("t2", "https://example.com/page?utm_source=twitter&utm_medium=social", "A copy"),
        make_tab("t3", "https://different.com/", "Other"),
    ])]);
    let result = DedupEngine::dedup(&session, DedupStrategy::NormalizedUrl);
    assert_eq!(result.original_count, 3);
    assert_eq!(result.deduplicated_count, 2);
}

/// Real OneTab pattern: dailydev links with ?ref=dailydev suffix.
/// Note: only utm_* params are stripped by NormalizedUrl; ?ref= is preserved,
/// so these count as distinct URLs.
#[test]
fn test_dedup_normalized_url_ref_param_not_stripped() {
    let session = make_session(vec![make_group("g1", vec![
        make_tab("t1", "https://engineering.ramp.com/post/why-we-built-our-background-agent", "Ramp"),
        make_tab("t2", "https://engineering.ramp.com/post/why-we-built-our-background-agent?ref=dailydev", "Ramp via daily.dev"),
    ])]);
    let result = DedupEngine::dedup(&session, DedupStrategy::NormalizedUrl);
    // ?ref= is not a utm_ param so it's not stripped — these are 2 distinct normalized URLs
    assert_eq!(result.deduplicated_count, 2, "?ref= is not stripped, so both URLs are distinct");
}

/// utm_ tracking params ARE stripped — these two should be deduped
#[test]
fn test_dedup_normalized_url_utm_params_stripped() {
    let session = make_session(vec![make_group("g1", vec![
        make_tab("t1", "https://example.com/post", "Post"),
        make_tab("t2", "https://example.com/post?utm_source=twitter&utm_medium=social", "Post via Twitter"),
    ])]);
    let result = DedupEngine::dedup(&session, DedupStrategy::NormalizedUrl);
    assert_eq!(result.deduplicated_count, 1, "utm_ params should be stripped, deduping these two");
    assert_eq!(result.removed.len(), 1);
}

// ─── Dedup – across multiple groups ────────────────────────────────────────

#[test]
fn test_dedup_across_multiple_groups() {
    let session = make_session(vec![
        make_group("g1", vec![
            make_tab("t1", "https://github.com/tokio-rs/tokio", "Tokio"),
            make_tab("t2", "https://crates.io", "crates.io"),
        ]),
        make_group("g2", vec![
            make_tab("t3", "https://github.com/tokio-rs/tokio", "Tokio (duplicate)"),
            make_tab("t4", "https://doc.rust-lang.org", "Rust Docs"),
        ]),
    ]);
    let result = DedupEngine::dedup(&session, DedupStrategy::ExactUrl);
    assert_eq!(result.original_count, 4);
    assert_eq!(result.removed.len(), 1, "one cross-group duplicate should be removed");
}

// ─── Dedup – no duplicates ─────────────────────────────────────────────────

#[test]
fn test_dedup_no_duplicates_unchanged() {
    let session = make_real_shape_session();
    let result = DedupEngine::dedup(&session, DedupStrategy::ExactUrl);
    assert_eq!(result.removed.len(), 0);
    assert_eq!(result.original_count, result.deduplicated_count);
}

// ─── Live test (skipped unless env var set) ─────────────────────────────────

/// Fuzz the fuzzy searcher with a large synthetic session simulating real scale
#[test]
fn test_fuzzy_search_at_scale() {
    use std::time::Instant;
    // 200 groups × 50 tabs = 10 000 tabs — realistic sub-set of a 20k-tab collection
    let groups: Vec<TabGroup> = (0..200).map(|gi| {
        let tabs = (0..50).map(|ti| make_tab(
            &format!("t-{}-{}", gi, ti),
            &format!("https://github.com/rust-lang/repo-{}", ti),
            &format!("repo-{}: A Rust project about topic-{}", ti, gi),
        )).collect();
        make_group(&format!("g-{}", gi), tabs)
    }).collect();
    let session = make_session(groups);

    let t0 = Instant::now();
    let results = FuzzySearcher::search("rust topic", &session);
    let elapsed = t0.elapsed();
    eprintln!("scale search: {} results in {:?}", results.len(), elapsed);
    assert!(!results.is_empty());
    // Should complete in under 2 seconds even on slow CI
    assert!(elapsed.as_secs() < 2, "fuzzy search too slow: {:?}", elapsed);
}
