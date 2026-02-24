//! OneTab LevelDB recovery and import pipeline for tablitz.
//!
//! This crate provides functionality to:
//! - Resolve OneTab LevelDB paths across browsers and platforms
//! - Safely read from browser LevelDB stores (handling lock contention)
//! - Parse OneTab's internal JSON schema from LevelDB
//! - Import from OneTab export files (both pipe-separated and markdown formats)
//! - Provide a CLI-accessible API for tab recovery

use anyhow::{Context, Result};
use chrono::Utc;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use tablitz_core::{ms_timestamp_to_datetime, SessionSource, Tab, TabGroup, TabSession};
use tempfile::TempDir;
use rusty_leveldb::LdbIterator;

/// Supported browsers for OneTab extension recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Browser {
    /// Google Chrome
    Chrome,
    /// Microsoft Edge
    Edge,
    /// Brave Browser
    Brave,
    /// Perplexity Comet
    Comet,
}

impl Browser {
    /// Returns the OneTab extension ID for this browser.
    ///
    /// Chrome, Brave, and Comet (Chromium-based browsers) share the same
    /// Web Store extension ID. Edge has a separate ID due to Microsoft's
    /// Edge Add-ons store.
    pub fn onetab_extension_id(&self) -> &'static str {
        match self {
            Browser::Chrome | Browser::Brave | Browser::Comet => {
                "chphlpgkkbolifaimnlloiipkdnihall"
            }
            Browser::Edge => "hoimpamkkoehapgenciaoajfkfkpgfop",
        }
    }

    /// Returns the human-readable display name for this browser.
    pub fn display_name(&self) -> &'static str {
        match self {
            Browser::Chrome => "Chrome",
            Browser::Edge => "Edge",
            Browser::Brave => "Brave",
            Browser::Comet => "Comet (Perplexity)",
        }
    }
}

/// Resolve the path to a browser's OneTab LevelDB store for a given profile.
///
/// # Browser-specific Paths
///
/// **Windows:**
/// ```text
/// %LOCALAPPDATA%\<vendor>\<browser>\User Data\<profile>\Local Extension Settings\<ext_id>
/// ```
///
/// **macOS:**
/// ```text
/// ~/Library/Application Support/<vendor>/<browser>/<profile>/Local Extension Settings/<ext_id>
/// ```
///
/// **Linux:**
/// ```text
/// ~/.config/<vendor-lowercase>/<profile>/Local Extension Settings/<ext_id>
/// ```
///
/// # Vendor/Browser Directory Names
///
/// | Browser | Windows | macOS | Linux |
/// |---------|---------|-------|-------|
/// | Chrome | `Google\Chrome` | `Google/Chrome` | `google-chrome` |
/// | Edge | `Microsoft\Edge` | `Microsoft/Edge` | `microsoft-edge` |
/// | Brave | `BraveSoftware\Brave-Browser` | `BraveSoftware/Brave-Browser` | `BraveSoftware/Brave-Browser` |
/// | Comet | `Perplexity\Comet` | `Perplexity/Comet` | `perplexity-comet` |
///
/// # Errors
///
/// Returns an error if the platform is unsupported, the base directory cannot be found,
/// or path construction fails.
pub fn resolve_leveldb_path(browser: &Browser, profile: &str) -> Result<PathBuf> {
    let ext_id = browser.onetab_extension_id();

    #[cfg(windows)]
    {
        if let Some(data_dir) = dirs::data_local_dir() {
            return Ok(data_dir
                .join(browser_subdir(browser, "windows", false))
                .join(profile)
                .join("Local Extension Settings")
                .join(ext_id));
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home_dir) = dirs::home_dir() {
            return Ok(home_dir
                .join("Library")
                .join("Application Support")
                .join(browser_subdir(browser, "macos", true))
                .join(profile)
                .join("Local Extension Settings")
                .join(ext_id));
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(config_dir) = dirs::config_dir() {
            return Ok(config_dir
                .join(browser_subdir(browser, "linux", false))
                .join(profile)
                .join("Local Extension Settings")
                .join(ext_id));
        }
    }

    Err(anyhow::anyhow!("Unsupported platform or could not find base directory"))
}

/// Returns the browser's vendor/browser directory name for the platform.
fn browser_subdir(browser: &Browser, platform: &str, vendor_first: bool) -> PathBuf {
    let (vendor, browser_name) = match browser {
        Browser::Chrome => ("Google", "Chrome"),
        Browser::Edge => ("Microsoft", "Edge"),
        Browser::Brave => ("BraveSoftware", "Brave-Browser"),
        Browser::Comet => ("Perplexity", "Comet"),
    };

    if platform == "linux" {
        // Linux uses lowercase vendor_browser format
        let parts = if vendor_first {
            format!("{}/{}", vendor.to_lowercase(), browser_name.to_lowercase())
        } else {
            vendor.to_lowercase()
        };
        PathBuf::from(parts)
    } else if platform == "windows" {
        // Windows uses backslash-separated format
        PathBuf::from(format!("{}\\{}", vendor, browser_name))
    } else {
        // macOS uses forward slash-separated format
        PathBuf::from(format!("{}/{}", vendor, browser_name))
    }
}

/// Open a LevelDB database, handling lock contention gracefully.
///
/// If the database is locked (browser is open), logs a warning and copies to
/// a temp directory first, then reads from the copy. This prevents the "database
/// is locked" issue while keeping the process non-blocking.
///
/// # Returns
///
/// A tuple of:
/// - The opened LevelDB database
/// - An optional `TempDir` that holds the copy (if one was made)
///
/// The `TempDir` is returned to ensure the temporary copy is cleaned up when
/// dropped, but is optional since we return `None` when reading directly from
/// the original path without copying.
///
/// # Errors
///
/// Returns an error if:
/// - The LevelDB cannot be opened (even after copying to temp)
/// - The directory cannot be copied (e.g., permission issues)
fn open_leveldb_safe(path: &Path) -> Result<(rusty_leveldb::DB, Option<TempDir>)> {
    let opts = rusty_leveldb::Options::default();

    // Try opening directly first
    match rusty_leveldb::DB::open(path, opts.clone()) {
        Ok(db) => Ok((db, None)),
        Err(e) => {
            // Check if it's a lock-related error
            let error_msg = format!("{:?}", e);
            if error_msg.contains("lock") || error_msg.contains("Locked") {
                eprintln!(
                    "warning: LevelDB locked at {}, copying to temp dir...",
                    path.display()
                );

                // Create temp directory and copy the LevelDB
                let temp_dir = tempfile::tempdir().context("Failed to create temp directory")?;
                let temp_path = temp_dir.path();

                copy_dir_recursive(path, temp_path).context("Failed to copy LevelDB to temp dir")?;

                // Try opening from the copy
                let db = rusty_leveldb::DB::open(temp_path.join("leveldb"), opts)
                    .context("Failed to open copied LevelDB")?;

                Ok((db, Some(temp_dir)))
            } else {
                // Some other error, not a lock issue
                Err(anyhow::anyhow!("Failed to open LevelDB: {}", e))
            }
        }
    }
}

/// Recursively copies a directory from src to dst.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    // Create the destination directory inside temp_dir
    let leveldb_dst = dst.join("leveldb");
    fs::create_dir_all(&leveldb_dst).context("Failed to create temp LevelDB directory")?;

    for entry in fs::read_dir(src).context("Failed to read source directory")? {
        let entry = entry.context("Failed to read directory entry")?;
        let file_type = entry.file_type().context("Failed to get file type")?;

        let src_path = entry.path();
        let dst_path = leveldb_dst.join(entry.file_name());

        if file_type.is_file() {
            fs::copy(&src_path, &dst_path).context("Failed to copy file")?;
        } else if file_type.is_dir() {
            // Recursively copy subdirectories
            copy_dir_recursive(&src_path, &dst_path)?;
        }
    }

    Ok(())
}

mod onetab_schema {
    use super::*;

    /// OneTab's root JSON schema from LevelDB values.
    #[derive(Deserialize, Debug)]
    pub struct OneTabRoot {
        #[serde(rename = "tabGroups")]
        pub tab_groups: Vec<OneTabGroup>,
    }

    /// A tab group in OneTab's internal schema.
    #[derive(Deserialize, Debug)]
    pub struct OneTabGroup {
        pub id: String,
        #[serde(rename = "tabsMeta")]
        pub tabs_meta: Vec<OneTabTab>,
        #[serde(rename = "createDate")]
        pub create_date: i64,
        pub title: Option<String>,
        pub pinned: Option<bool>,
        pub locked: Option<bool>,
        pub starred: Option<bool>,
    }

    /// A tab in OneTab's internal schema.
    #[derive(Deserialize, Debug)]
    pub struct OneTabTab {
        pub id: String,
        pub url: String,
        pub title: String,
        #[serde(rename = "favicon")]
        pub favicon_url: Option<String>,
    }
}

/// Parse OneTab's JSON schema from a LevelDB value and extract a TabSession.
///
/// Iterates all key-value pairs in the LevelDB, finds entries where the value
/// contains "tabGroups" (indicating it's OneTab's main data structure), parses
/// the JSON, and converts it to a TabSession.
///
/// # Arguments
///
/// * `path` - Path to the LevelDB directory
/// * `source` - The SessionSource describing where this data came from
///
/// # Returns
///
/// A `TabSession` containing all tab groups found in the LevelDB.
///
/// # Errors
///
/// Returns an error if:
/// - The LevelDB cannot be opened
/// - JSON parsing fails
/// - URL parsing fails (invalid URLs are skipped with a warning)
pub fn extract_from_leveldb(path: &Path, source: SessionSource) -> Result<TabSession> {
    let (mut db, _temp_dir) = open_leveldb_safe(path)?;

    let mut iter = db.new_iter()?;
    let mut key = Vec::new();
    let mut value = Vec::new();
    let mut found_groups = Vec::new();
    let mut all_tabs_count = 0;

    iter.advance();
    while iter.valid() {
        iter.current(&mut key, &mut value);

        if let Ok(value_str) = std::str::from_utf8(&value) {
            // Look for OneTab's tabGroups structure
            if value_str.contains("tabGroups") {
                if let Ok(root) = serde_json::from_str::<onetab_schema::OneTabRoot>(value_str) {
                    for group in root.tab_groups {
                        // Parse URLs, skipping invalid ones
                        let tabs: Vec<Tab> = group
                            .tabs_meta
                            .into_iter()
                            .filter_map(|t| {
                                match url::Url::parse(&t.url) {
                                    Ok(parsed_url) => Some(Tab {
                                        id: t.id,
                                        url: parsed_url,
                                        title: t.title,
                                        favicon_url: t.favicon_url,
                                        added_at: ms_timestamp_to_datetime(group.create_date),
                                    }),
                                    Err(e) => {
                                        eprintln!(
                                            "warning: skipping invalid URL '{}': {}",
                                            t.url, e
                                        );
                                        None
                                    }
                                }
                            })
                            .collect();

                        if !tabs.is_empty() {
                            let tab_group = TabGroup {
                                id: group.id,
                                label: group.title,
                                created_at: ms_timestamp_to_datetime(group.create_date),
                                tabs,
                                pinned: group.pinned.unwrap_or(false),
                                locked: group.locked.unwrap_or(false),
                                starred: group.starred.unwrap_or(false),
                            };
                            let tab_count = tab_group.tabs.len();
                            found_groups.push(tab_group);
                            all_tabs_count += tab_count;
                        }
                    }
                }
            }
        }

        iter.advance();
    }

    eprintln!("Recovered {} tab groups, {} tabs total", found_groups.len(), all_tabs_count);

    Ok(TabSession {
        version: 1,
        source,
        groups: found_groups,
        created_at: Utc::now(), // We don't know the original creation time
        imported_at: Utc::now(),
    })
}

/// Format detection for OneTab export files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportFormat {
    /// OneTab's native pipe-separated format
    Pipe,
    /// tablitz/JS-gist markdown format
    Markdown,
}

/// Parse a OneTab export file, detecting format automatically.
///
/// Supports two formats:
///
/// **Format A - Pipe Format (OneTab native):**
/// ```text
/// https://url.com | Tab Title
/// https://url2.com | Tab Title 2
///
/// https://url3.com | Tab Title 3
/// ```
/// Empty lines separate tab groups. No timestamps. No group labels.
///
/// **Format B - Markdown:**
/// ```markdown
/// ---
/// ## 8 tabs
/// > Created 3/20/2025, 10:08:46 PM
///
/// [Tab Title](https://url.com)
/// [Tab Title 2](https://url2.com)
/// ```
///
/// # Arguments
///
/// * `path` - Path to the export file
///
/// # Returns
///
/// A `TabSession` containing the tab groups from the export file.
///
/// # Errors
///
/// Returns an error if:
/// - The file cannot be read
/// - The format cannot be determined
/// - Parsing fails
pub fn parse_onetab_export(path: &Path) -> Result<TabSession> {
    let content = fs::read_to_string(path).context("Failed to read export file")?;

    let format = detect_format(&content);

    let groups = match format {
        ExportFormat::Pipe => parse_pipe_format(&content)?,
        ExportFormat::Markdown => parse_markdown_format(&content)?,
    };

    let source = SessionSource::OneTabExport {
        path: path.to_string_lossy().to_string(),
    };

    eprintln!(
        "Imported {} tab groups from {} (format: {:?})",
        groups.len(),
        path.display(),
        format
    );

    Ok(TabSession {
        version: 1,
        source,
        groups,
        created_at: Utc::now(),
        imported_at: Utc::now(),
    })
}

/// Detect the format of a OneTab export file.
fn detect_format(content: &str) -> ExportFormat {
    // Markdown format has characteristic "---" and "## X tabs" patterns
    if content.contains("---") && content.contains("##") && content.contains("tabs") {
        return ExportFormat::Markdown;
    }

    // Default to pipe format (OneTab's native export)
    ExportFormat::Pipe
}

/// Parse OneTab's pipe-separated export format.
///
/// Format: `url | title` with blank lines separating groups.
fn parse_pipe_format(content: &str) -> Result<Vec<TabGroup>> {
    let mut groups = Vec::new();
    let mut current_tabs = Vec::new();
    let mut group_index = 0;

    for line in content.lines() {
        let line = line.trim();

        // Skip empty lines - these separate groups
        if line.is_empty() {
            if !current_tabs.is_empty() {
                let group = TabGroup {
                    id: format!("pipe-import-{}", group_index),
                    label: None,
                    created_at: Utc::now(), // Timestamp not available in pipe format
                    tabs: current_tabs,
                    pinned: false,
                    locked: false,
                    starred: false,
                };
                groups.push(group);
                current_tabs = Vec::new();
                group_index += 1;
            }
            continue;
        }

        // Parse "url | title" format
        if let Some((url_part, title_part)) = line.split_once('|') {
            let url_str = url_part.trim();
            let title = title_part.trim().to_string();

            if let Ok(parsed_url) = url::Url::parse(url_str) {
                current_tabs.push(Tab {
                    id: format!("tab-{}-{}", group_index, current_tabs.len()),
                    url: parsed_url,
                    title,
                    favicon_url: None,
                    added_at: Utc::now(), // Timestamp not available
                });
            } else {
                eprintln!("warning: skipping invalid URL in pipe format: '{}'", url_str);
            }
        }
    }

    // Don't forget the last group
    if !current_tabs.is_empty() {
        let group = TabGroup {
            id: format!("pipe-import-{}", group_index),
            label: None,
            created_at: Utc::now(),
            tabs: current_tabs,
            pinned: false,
            locked: false,
            starred: false,
        };
        groups.push(group);
    }

    Ok(groups)
}

/// Parse the markdown format (from the JS-gist script).
fn parse_markdown_format(content: &str) -> Result<Vec<TabGroup>> {
    let mut groups = Vec::new();
    let mut current_header: Option<(String, String)> = None; // (title, timestamp_str)
    let mut current_tabs = Vec::new();
    let mut group_index = 0;

    for line in content.lines().map(|l| l.trim()).collect::<Vec<_>>() {
        // Group separator
        if line == "---" {
            // Save previous group if exists
            if !current_tabs.is_empty() {
                let (label, _timestamp) = current_header.take().unwrap_or_default();

                // Try to parse timestamp from header if available
                let created_at = Utc::now(); // Could parse timestamp from markdown header

                let group = TabGroup {
                    id: format!("markdown-import-{}", group_index),
                    label: if label.is_empty() { None } else { Some(label) },
                    created_at,
                    tabs: current_tabs,
                    pinned: false,
                    locked: false,
                    starred: false,
                };
                groups.push(group);
                current_tabs = Vec::new();
                group_index += 1;
            }
            current_header = None;
            continue;
        }

        // Group header: "## X tabs" or similar
        if let Some(stripped) = line.strip_prefix("##") {
            let title = stripped.trim().to_string();
            current_header = Some((title, String::new()));
            continue;
        }

        // Timestamp line: "> Created ..."
        if line.starts_with(">") {
            if let Some(header) = &mut current_header {
                header.1 = line.strip_prefix('>').unwrap_or("").trim().to_string();
            }
            continue;
        }

        // Tab link: "[Title](url)"
        if let Some(rest) = line.strip_prefix('[') {
            if let Some((title_part, url_part)) = rest.split_once("](") {
                if let Some(url_str) = url_part.strip_suffix(')') {
                    let title = title_part.to_string();

                    if let Ok(parsed_url) = url::Url::parse(url_str) {
                        current_tabs.push(Tab {
                            id: format!("tab-{}-{}", group_index, current_tabs.len()),
                            url: parsed_url,
                            title,
                            favicon_url: None,
                            added_at: Utc::now(),
                        });
                    } else {
                        eprintln!(
                            "warning: skipping invalid URL in markdown format: '{}'",
                            url_str
                        );
                    }
                }
            }
        }
    }

    // Last group
    if !current_tabs.is_empty() {
        let (label, _timestamp) = current_header.take().unwrap_or_default();
        let group = TabGroup {
            id: format!("markdown-import-{}", group_index),
            label: if label.is_empty() { None } else { Some(label) },
            created_at: Utc::now(),
            tabs: current_tabs,
            pinned: false,
            locked: false,
            starred: false,
        };
        groups.push(group);
    }

    Ok(groups)
}

/// Configuration options for the recovery process.
#[derive(Debug, Clone)]
pub struct RecoverOptions {
    /// The browser to recover from
    pub browser: Browser,
    /// The profile name (e.g., "Default", "Profile 1")
    pub profile: String,
    /// If true, only validates the path without reading data
    pub dry_run: bool,
    /// Optional override for the auto-resolved LevelDB path
    pub db_path: Option<PathBuf>,
}

impl Default for RecoverOptions {
    fn default() -> Self {
        Self {
            browser: Browser::Chrome,
            profile: "Default".to_string(),
            dry_run: false,
            db_path: None,
        }
    }
}

/// Run the full recovery pipeline for a single browser's OneTab store.
///
/// If `db_path` is not specified in options, automatically resolves the path
/// for the given browser and profile. Handles lock contention by copying to
/// a temporary directory if needed.
///
/// # Arguments
///
/// * `opts` - Recovery configuration options
///
/// # Returns
///
/// A `TabSession` containing all recovered tab groups.
///
/// # Errors
///
/// Returns an error if:
/// - The LevelDB path cannot be resolved (when not overridden)
/// - The path does not exist or is not a directory
/// - The LevelDB cannot be opened (even after handling lock contention)
/// - Data parsing fails
pub fn recover(opts: RecoverOptions) -> Result<TabSession> {
    let db_path = if let Some(custom_path) = opts.db_path {
        custom_path
    } else {
        resolve_leveldb_path(&opts.browser, &opts.profile)?
    };

    if opts.dry_run {
        println!(
            "Dry run: Would read from {}",
            db_path.display()
        );
        return Ok(TabSession {
            version: 1,
            source: match opts.browser {
                Browser::Chrome => SessionSource::Chrome {
                    profile: opts.profile.clone(),
                },
                Browser::Edge => SessionSource::Edge {
                    profile: opts.profile.clone(),
                },
                Browser::Brave => SessionSource::Brave {
                    profile: opts.profile.clone(),
                },
                Browser::Comet => SessionSource::Comet {
                    profile: opts.profile.clone(),
                },
            },
            groups: Vec::new(),
            created_at: Utc::now(),
            imported_at: Utc::now(),
        });
    }

    if !db_path.exists() {
        return Err(anyhow::anyhow!(
            "OneTab LevelDB path does not exist: {}",
            db_path.display()
        ));
    }

    if !db_path.is_dir() {
        return Err(anyhow::anyhow!(
            "OneTab LevelDB path is not a directory: {}",
            db_path.display()
        ));
    }

    let source = match opts.browser {
        Browser::Chrome => SessionSource::Chrome {
            profile: opts.profile.clone(),
        },
        Browser::Edge => SessionSource::Edge {
            profile: opts.profile.clone(),
        },
        Browser::Brave => SessionSource::Brave {
            profile: opts.profile.clone(),
        },
        Browser::Comet => SessionSource::Comet {
            profile: opts.profile.clone(),
        },
    };

    extract_from_leveldb(&db_path, source)
}

/// List all auto-detected OneTab LevelDB paths on this system across all supported browsers.
///
/// Attempts to resolve the default profile ("Default") for each browser and returns
/// all paths that exist. This is useful for discovering which browsers have OneTab
/// installed and have data available for recovery.
///
/// # Returns
///
/// A vector of tuples containing (browser, profile, path) for each detected store.
pub fn detect_all_onetab_stores() -> Vec<(Browser, String, PathBuf)> {
    let browsers = [
        Browser::Chrome,
        Browser::Edge,
        Browser::Brave,
        Browser::Comet,
    ];

    let mut detected = Vec::new();

    for browser in &browsers {
        // Try common profile names
        let profiles = vec!["Default".to_string(), "Profile 1".to_string()];

        for profile in &profiles {
            if let Ok(path) = resolve_leveldb_path(browser, profile) {
                if path.exists() && path.is_dir() {
                    detected.push((*browser, profile.clone(), path));
                }
            }
        }
    }

    detected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_browser_extension_ids() {
        assert_eq!(
            Browser::Chrome.onetab_extension_id(),
            "chphlpgkkbolifaimnlloiipkdnihall"
        );
        assert_eq!(
            Browser::Brave.onetab_extension_id(),
            "chphlpgkkbolifaimnlloiipkdnihall"
        );
        assert_eq!(
            Browser::Edge.onetab_extension_id(),
            "hoimpamkkoehapgenciaoajfkfkpgfop"
        );
    }

    #[test]
    fn test_browser_display_names() {
        assert_eq!(Browser::Chrome.display_name(), "Chrome");
        assert_eq!(Browser::Edge.display_name(), "Edge");
        assert!(Browser::Comet.display_name().contains("Perplexity"));
    }

    #[test]
    fn test_format_detection() {
        let pipe_content = "https://example.com | Example Site\n\nhttps://other.com | Other";
        assert_eq!(detect_format(pipe_content), ExportFormat::Pipe);

        let markdown_content = r#"---
## 2 tabs
> Created 3/20/2025

[Example Site](https://example.com)
"#;
        assert_eq!(detect_format(markdown_content), ExportFormat::Markdown);
    }

    #[test]
    fn test_parse_pipe_format() {
        let content = r#"https://example.com | Example Site
https://other.com | Other Site

https://example.org | Third Site"#;
        let groups = parse_pipe_format(content).unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].tabs.len(), 2);
        assert_eq!(groups[1].tabs.len(), 1);
        assert_eq!(groups[0].tabs[0].url.as_str(), "https://example.com/");
        assert_eq!(groups[1].tabs[0].title, "Third Site");
    }

    #[test]
    fn test_parse_markdown_format() {
        let content = r#"---
## 2 tabs
> Created 3/20/2025, 10:08:46 PM

[Example Site](https://example.com)
[Other Site](https://other.com)
"#;
        let groups = parse_markdown_format(content).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].tabs.len(), 2);
        assert_eq!(groups[0].tabs[0].title, "Example Site");
        assert_eq!(groups[0].tabs[0].url.as_str(), "https://example.com/");
    }

    #[test]
    fn test_recover_options_default() {
        let opts = RecoverOptions::default();
        assert!(matches!(opts.browser, Browser::Chrome));
        assert_eq!(opts.profile, "Default");
        assert!(!opts.dry_run);
        assert!(opts.db_path.is_none());
    }
}
