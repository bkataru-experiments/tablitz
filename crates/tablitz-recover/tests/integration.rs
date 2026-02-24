use tablitz_recover::parse_onetab_export;

#[test]
fn test_parse_pipe_format() {
    let content = "https://example.com/one | First Tab\nhttps://example.com/two | Second Tab\n\nhttps://example.com/three | Third Tab\n";
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), content).unwrap();
    let session = parse_onetab_export(tmp.path()).unwrap();
    assert_eq!(session.groups.len(), 2);
    assert_eq!(session.groups[0].tabs.len(), 2);
    assert_eq!(session.groups[1].tabs.len(), 1);
    assert_eq!(session.groups[0].tabs[0].url.as_str(), "https://example.com/one");
    assert_eq!(session.groups[0].tabs[0].title, "First Tab");
}

#[test]
fn test_parse_markdown_format() {
    let content = "---\n## 2 tabs\n> Created 3/20/2025, 10:08:46 PM\n\n[Tab One](https://example.com/one)\n[Tab Two](https://example.com/two)\n\n";
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), content).unwrap();
    let session = parse_onetab_export(tmp.path()).unwrap();
    assert_eq!(session.groups.len(), 1);
    assert_eq!(session.groups[0].tabs.len(), 2);
    assert_eq!(session.groups[0].tabs[0].title, "Tab One");
}

#[test]
fn test_stable_ids_same_content() {
    let content = "https://example.com/one | First Tab\n";
    let tmp1 = tempfile::NamedTempFile::new().unwrap();
    let tmp2 = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp1.path(), content).unwrap();
    std::fs::write(tmp2.path(), content).unwrap();
    let s1 = parse_onetab_export(tmp1.path()).unwrap();
    let s2 = parse_onetab_export(tmp2.path()).unwrap();
    assert_eq!(s1.groups[0].id, s2.groups[0].id);
    assert_eq!(s1.groups[0].tabs[0].id, s2.groups[0].tabs[0].id);
}
