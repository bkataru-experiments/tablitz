use tablitz_recover::parse_onetab_export;

// ─── Helpers ───────────────────────────────────────────────────────────────

fn write_tmp(content: &str) -> tempfile::NamedTempFile {
    let f = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(f.path(), content).unwrap();
    f
}

// ─── Pipe format – basic ───────────────────────────────────────────────────

#[test]
fn test_parse_pipe_format_two_groups() {
    let content = "https://example.com/one | First Tab\nhttps://example.com/two | Second Tab\n\nhttps://example.com/three | Third Tab\n";
    let f = write_tmp(content);
    let session = parse_onetab_export(f.path()).unwrap();
    assert_eq!(session.groups.len(), 2);
    assert_eq!(session.groups[0].tabs.len(), 2);
    assert_eq!(session.groups[1].tabs.len(), 1);
    assert_eq!(session.groups[0].tabs[0].url.as_str(), "https://example.com/one");
    assert_eq!(session.groups[0].tabs[0].title, "First Tab");
}

#[test]
fn test_parse_pipe_format_single_group() {
    let content = "https://doc.rust-lang.org/book/ | The Rust Programming Language\nhttps://crates.io | crates.io: Rust Package Registry\n";
    let f = write_tmp(content);
    let session = parse_onetab_export(f.path()).unwrap();
    assert_eq!(session.groups.len(), 1);
    assert_eq!(session.groups[0].tabs.len(), 2);
}

/// Real OneTab pipe export: unicode in titles (actual user data shape)
#[test]
fn test_parse_pipe_format_unicode_titles() {
    let content = concat!(
        "https://x.com/baalatejakataru/status/1902846354617143514 | #pragma omp ⟨ε|Δ⟩ on X: \"lol i was so heated\"\n",
        "https://x.com/baalatejakataru/status/1786215969058324930 | #pragma omp ⟨ε|Δ⟩ on X: \"mark my words\"\n",
    );
    let f = write_tmp(content);
    let session = parse_onetab_export(f.path()).unwrap();
    assert_eq!(session.groups.len(), 1);
    assert_eq!(session.groups[0].tabs.len(), 2);
    assert!(session.groups[0].tabs[0].title.contains("⟨ε|Δ⟩"));
}

/// Real OneTab pipe export: very long Google Search URLs
#[test]
fn test_parse_pipe_format_long_urls() {
    let long_url = "https://www.google.com/search?q=how+do+I+use+scp%3F&oq=scp&sourceid=chrome&ie=UTF-8&udm=50&aep=48&cud=0&qsubts=1771704930126&source=chrome.crn.obic&mstk=AUtExfA5nmxqURL";
    let content = format!("{} | how do I use scp? - Google Search\n", long_url);
    let f = write_tmp(content.as_str());
    let session = parse_onetab_export(f.path()).unwrap();
    assert_eq!(session.groups[0].tabs[0].url.as_str(), long_url);
}

/// Pipe format: tab without title (URL only, no |)
#[test]
fn test_parse_pipe_format_url_only() {
    let content = "https://example.com\nhttps://other.com | With Title\n";
    let f = write_tmp(content);
    // Should not panic; URL-only lines may be skipped or kept depending on impl
    let _ = parse_onetab_export(f.path());
}

/// Pipe format: description header lines (non-URL text before tabs)
#[test]
fn test_parse_pipe_format_description_header() {
    // OneTab export files sometimes have text before the first URL block
    let content = concat!(
        "this is onetab's default export format, here is an example\n",
        "\n",
        "https://example.com/one | First Tab\n",
        "https://example.com/two | Second Tab\n",
    );
    let f = write_tmp(content);
    let session = parse_onetab_export(f.path()).unwrap();
    // The description header should be ignored; only URL lines are tabs
    let total_tabs: usize = session.groups.iter().map(|g| g.tabs.len()).sum();
    assert_eq!(total_tabs, 2);
}

/// Multiple groups from real daily.dev / GitHub tab data shape
#[test]
fn test_parse_pipe_format_real_data_shape() {
    let content = concat!(
        "https://github.com/code-yeongyu/oh-my-opencode | oh-my-opencode: the best agent harness\n",
        "https://build.nvidia.com/models | Try NVIDIA NIM APIs\n",
        "https://github.com/bkataru | bkataru (Baalateja Kataru)\n",
        "\n",
        "https://arxiv.org/abs/1905.11946 | EfficientNet: Rethinking Model Scaling\n",
        "https://en.wikipedia.org/wiki/EfficientNet | EfficientNet - Wikipedia\n",
        "\n",
        "https://mail.google.com/mail/u/0/#inbox | Inbox (385) - Gmail\n",
    );
    let f = write_tmp(content);
    let session = parse_onetab_export(f.path()).unwrap();
    assert_eq!(session.groups.len(), 3);
    assert_eq!(session.groups[0].tabs.len(), 3);
    assert_eq!(session.groups[1].tabs.len(), 2);
    assert_eq!(session.groups[2].tabs.len(), 1);
}

// ─── Markdown format – basic ────────────────────────────────────────────────

#[test]
fn test_parse_markdown_format_single_group_no_label() {
    let content = concat!(
        "---\n",
        "## 2 tabs\n",
        "> Created 3/20/2025, 10:08:46 PM\n",
        "\n",
        "[Tab One](https://example.com/one)\n",
        "[Tab Two](https://example.com/two)\n",
        "\n",
    );
    let f = write_tmp(content);
    let session = parse_onetab_export(f.path()).unwrap();
    assert_eq!(session.groups.len(), 1);
    assert_eq!(session.groups[0].tabs.len(), 2);
    assert_eq!(session.groups[0].tabs[0].title, "Tab One");
    // Regression: "## N tabs" must NOT be used as label
    assert!(session.groups[0].label.is_none(), "label should be None, not '2 tabs'");
}

/// Real markdown format from the JS-gist export script (actual user data shape)
#[test]
fn test_parse_markdown_format_real_data_shape() {
    let content = concat!(
        "---\n",
        "## 8 tabs\n",
        "> Created 3/20/2025, 10:08:46 PM\n",
        "\n",
        "[notebook lm clone - Google Search](https://www.google.com/search?q=notebook+lm+clone)\n",
        "[Make a NotebookLM Clone with Open Weights - YouTube](https://www.youtube.com/watch?v=DtisQDXLnXk)\n",
        "[How I Developed a NotebookLM Clone? | Towards AI](https://pub.towardsai.net/how-i-developed-a-notebooklm-clone-2d901d1c72a6)\n",
        "[gabrielchua/open-notebooklm](https://github.com/gabrielchua/open-notebooklm/tree/main)\n",
        "[lfnovo/open-notebook](https://github.com/lfnovo/open-notebook/tree/main)\n",
        "[Get Started | Open Notebook](https://www.open-notebook.ai/get-started.html)\n",
        "[What is Open Notebook?](https://www.open-notebook.ai/)\n",
        "[Meet Open NotebookLM](https://itsfoss.com/open-notebooklm/)\n",
        "\n",
        "---\n",
        "## 4 tabs\n",
        "> Created 3/20/2025, 12:58:31 AM\n",
        "\n",
        "[Data-Oriented Design - Games from Within](https://gamesfromwithin.com/data-oriented-design)\n",
        "[CppCon 2014: Mike Acton - YouTube](https://www.youtube.com/watch?v=rX0ItVEVjHc)\n",
        "[[1710.03462] SoAx: generic C++ Structure of Arrays](https://arxiv.org/abs/1710.03462)\n",
        "[What's wrong?](https://www.dataorienteddesign.com/dodmain/node17.html)\n",
        "\n",
    );
    let f = write_tmp(content);
    let session = parse_onetab_export(f.path()).unwrap();
    assert_eq!(session.groups.len(), 2, "expected 2 groups");
    assert_eq!(session.groups[0].tabs.len(), 8);
    assert_eq!(session.groups[1].tabs.len(), 4);
    // All labels should be None (no second "> " line)
    for g in &session.groups {
        assert!(g.label.is_none(), "real markdown export has no user labels");
    }
}

/// Markdown with user label (second "> " line after Created timestamp)
#[test]
fn test_parse_markdown_format_with_user_label() {
    let content = concat!(
        "---\n",
        "## 2 tabs\n",
        "> Created 3/20/2025, 10:08:46 PM\n",
        "> Rust research\n",
        "\n",
        "[The Rust Programming Language](https://doc.rust-lang.org/book/)\n",
        "[Cargo Package Manager](https://doc.rust-lang.org/cargo/)\n",
        "\n",
    );
    let f = write_tmp(content);
    let session = parse_onetab_export(f.path()).unwrap();
    assert_eq!(session.groups.len(), 1);
    assert_eq!(session.groups[0].label.as_deref(), Some("Rust research"));
}

/// Markdown with title containing special chars (pipe, brackets)
#[test]
fn test_parse_markdown_format_special_chars_in_title() {
    let content = concat!(
        "---\n",
        "## 2 tabs\n",
        "> Created 3/20/2025, 10:08:46 PM\n",
        "\n",
        "[[1905.11946] EfficientNet: Rethinking Model Scaling for CNNs](https://arxiv.org/abs/1905.11946)\n",
        "[crates.io | GitHub](https://crates.io)\n",
        "\n",
    );
    let f = write_tmp(content);
    let session = parse_onetab_export(f.path()).unwrap();
    assert_eq!(session.groups[0].tabs.len(), 2);
    // The title with [brackets] parsed correctly (at least URL is correct)
    assert_eq!(session.groups[0].tabs[1].url.as_str(), "https://crates.io/");
}

// ─── Format auto-detection ─────────────────────────────────────────────────

#[test]
fn test_format_detection_pipe() {
    let content = "https://example.com | Example Site\nhttps://other.com | Other\n";
    let f = write_tmp(content);
    let session = parse_onetab_export(f.path()).unwrap();
    // If detection works, pipe format was used (label is None, content parsed)
    let total: usize = session.groups.iter().map(|g| g.tabs.len()).sum();
    assert_eq!(total, 2);
}

#[test]
fn test_format_detection_markdown() {
    let content = concat!(
        "---\n",
        "## 3 tabs\n",
        "> Created 3/20/2025, 10:08:46 PM\n",
        "\n",
        "[A](https://a.com)\n",
        "[B](https://b.com)\n",
        "[C](https://c.com)\n",
    );
    let f = write_tmp(content);
    let session = parse_onetab_export(f.path()).unwrap();
    assert_eq!(session.groups[0].tabs.len(), 3);
}

// ─── Edge cases ────────────────────────────────────────────────────────────

#[test]
fn test_parse_empty_file() {
    let f = write_tmp("");
    let session = parse_onetab_export(f.path()).unwrap();
    assert_eq!(session.groups.len(), 0);
}

#[test]
fn test_parse_only_blank_lines() {
    let f = write_tmp("\n\n\n");
    let session = parse_onetab_export(f.path()).unwrap();
    assert_eq!(session.groups.len(), 0);
}

#[test]
fn test_parse_malformed_urls_skipped() {
    let content = "not-a-url | Bad Tab\nhttps://good.com | Good Tab\n";
    let f = write_tmp(content);
    // Should not panic; bad URL lines should be skipped
    let session = parse_onetab_export(f.path()).unwrap();
    let total: usize = session.groups.iter().map(|g| g.tabs.len()).sum();
    // At minimum good.com should be present
    assert!(total >= 1);
}

// ─── ID stability ──────────────────────────────────────────────────────────

#[test]
fn test_stable_ids_same_content() {
    let content = "https://example.com/one | First Tab\n";
    let f1 = write_tmp(content);
    let f2 = write_tmp(content);
    let s1 = parse_onetab_export(f1.path()).unwrap();
    let s2 = parse_onetab_export(f2.path()).unwrap();
    assert_eq!(s1.groups[0].id, s2.groups[0].id);
    assert_eq!(s1.groups[0].tabs[0].id, s2.groups[0].tabs[0].id);
}

#[test]
fn test_different_content_produces_different_ids() {
    let f1 = write_tmp("https://example.com/one | Tab A\n");
    let f2 = write_tmp("https://example.com/two | Tab B\n");
    let s1 = parse_onetab_export(f1.path()).unwrap();
    let s2 = parse_onetab_export(f2.path()).unwrap();
    assert_ne!(s1.groups[0].id, s2.groups[0].id);
}

// ─── Live test (skipped if env var not set) ─────────────────────────────────

#[test]
fn test_live_pipe_export_if_available() {
    let path = match std::env::var("TABLITZ_LIVE_PIPE_DATA") {
        Ok(p) => std::path::PathBuf::from(p),
        Err(_) => return, // skip
    };
    let session = parse_onetab_export(&path).expect("failed to parse live pipe data");
    assert!(!session.groups.is_empty(), "expected at least one group");
    let total: usize = session.groups.iter().map(|g| g.tabs.len()).sum();
    assert!(total > 0, "expected at least one tab");
    eprintln!("live: {} groups, {} tabs", session.groups.len(), total);
}

#[test]
fn test_live_markdown_export_if_available() {
    let path = match std::env::var("TABLITZ_LIVE_MD_DATA") {
        Ok(p) => std::path::PathBuf::from(p),
        Err(_) => return, // skip
    };
    let session = parse_onetab_export(&path).expect("failed to parse live markdown data");
    assert!(!session.groups.is_empty(), "expected at least one group");
    for g in &session.groups {
        // Regression: live markdown data must never have "N tabs" as label
        if let Some(label) = &g.label {
            let looks_like_tab_count = label.ends_with(" tabs") || label.ends_with(" tab");
            assert!(!looks_like_tab_count, "label should not be tab count: {:?}", label);
        }
    }
}

// ─── LevelDB double-encoding regression ────────────────────────────────────

/// Regression test: OneTab stores LevelDB values as double-encoded JSON strings.
/// The raw bytes are `"{\"tabGroups\":[...]}"` (outer JSON string wrapping inner JSON).
/// Previously, serde_json::from_str::<OneTabRoot> would silently fail on this format.
#[test]
fn test_extract_from_leveldb_double_encoded_value() {
    use tablitz_recover::extract_from_leveldb;
    use tablitz_core::SessionSource;

    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path();

    // Build a realistic OneTab inner JSON object
    let inner_json = r#"{"tabGroups":[{"id":"grp-test-001","createDate":1760074389851,"tabsMeta":[{"id":"tab-test-001","url":"https://example.com/rust","title":"The Rust Programming Language"},{"id":"tab-test-002","url":"https://docs.rs/serde","title":"serde - Rust"}]}]}"#;

    // Double-encode: serialize the JSON string as a JSON value (adds outer quotes + escaping)
    let outer_value = serde_json::to_string(inner_json).expect("double-encode");

    // Write to a real LevelDB
    let opts = rusty_leveldb::Options::default();
    let mut db = rusty_leveldb::DB::open(db_path, opts).expect("open db");
    db.put(b"state", outer_value.as_bytes()).expect("put");
    db.flush().expect("flush");
    drop(db);

    // Now recover — should find 1 group with 2 tabs
    let session = extract_from_leveldb(db_path, SessionSource::Unknown)
        .expect("extract_from_leveldb");

    assert_eq!(session.groups.len(), 1, "should find 1 group");
    assert_eq!(session.groups[0].tabs.len(), 2, "should find 2 tabs");
    assert_eq!(
        session.groups[0].tabs[0].title.as_str(),
        "The Rust Programming Language"
    );
    assert_eq!(
        session.groups[0].tabs[1].url.as_str(),
        "https://docs.rs/serde"
    );
}
