//! Core data models and types for tablitz.
//!
//! This crate provides the foundational types used throughout the tablitz workspace
//! for representing browser tabs, tab groups, and sessions. It is designed to be
//! wasm32-compatible and contains no OS-specific code.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

/// Converts a Unix millisecond timestamp to a DateTime<Utc>.
///
/// OneTab stores timestamps as milliseconds since epoch (e.g., 1760074389851).
/// This helper converts them to Rust's DateTime<Utc> for consistent handling.
pub fn ms_timestamp_to_datetime(ms: i64) -> DateTime<Utc> {
    let secs = ms / 1000;
    let nsecs = ((ms % 1000) * 1_000_000) as u32;
    DateTime::from_timestamp(secs, nsecs).unwrap_or_else(|| DateTime::from_timestamp(0, 0).unwrap())
}

/// A single saved browser tab.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct Tab {
    /// UUID or OneTab internal ID
    pub id: String,
    /// The URL of the tab
    pub url: Url,
    /// The title of the tab
    pub title: String,
    /// Optional favicon URL
    pub favicon_url: Option<String>,
    /// When this tab was added (from OneTab's createDate ms timestamp)
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub added_at: DateTime<Utc>,
}

impl Tab {
    /// Extracts the domain from the tab's URL.
    ///
    /// Returns `None` if the URL cannot have a domain (e.g., `about:blank`).
    pub fn domain(&self) -> Option<&str> {
        self.url.domain()
    }
}

/// A named group of tabs (what OneTab calls a "tab group").
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct TabGroup {
    /// Unique identifier for the group
    pub id: String,
    /// User-given name, may be None
    pub label: Option<String>,
    /// When this group was created
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub created_at: DateTime<Utc>,
    /// Tabs contained in this group
    pub tabs: Vec<Tab>,
    /// Whether the group is pinned
    pub pinned: bool,
    /// Whether the group is locked
    pub locked: bool,
    /// Whether the group is starred
    pub starred: bool,
}

impl TabGroup {
    /// Returns the number of tabs in this group.
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }
}

/// The source of tab/session data.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SessionSource {
    /// Chrome browser with specified profile
    Chrome { profile: String },
    /// Edge browser with specified profile
    Edge { profile: String },
    /// Brave browser with specified profile
    Brave { profile: String },
    /// Perplexity Comet browser with specified profile
    Comet { profile: String },
    /// Manual .txt export (pipe format or markdown)
    OneTabExport { path: String },
    /// tablitz's own JSON/TOML export
    TablitzNative { path: String },
    /// Unknown or unrecognized source
    Unknown,
}

/// A complete snapshot of tab data from one source.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct TabSession {
    /// Version of the tablitz data format
    pub version: u32,
    /// Where this data came from
    pub source: SessionSource,
    /// Tab groups contained in this session
    pub groups: Vec<TabGroup>,
    /// When the original data was captured
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub created_at: DateTime<Utc>,
    /// When tablitz ingested this data
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub imported_at: DateTime<Utc>,
}

/// Statistics about a session.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct SessionStats {
    /// Total number of groups in the session
    pub total_groups: usize,
    /// Total number of tabs across all groups
    pub total_tabs: usize,
    /// Earliest group creation time
    pub earliest_group: Option<DateTime<Utc>>,
    /// Latest group creation time
    pub latest_group: Option<DateTime<Utc>>,
    /// Top domains by tab count (domain, count)
    pub top_domains: Vec<(String, usize)>,
}

impl TabSession {
    /// Returns the total number of tabs across all groups.
    pub fn total_tab_count(&self) -> usize {
        self.groups.iter().map(|g| g.tab_count()).sum()
    }

    /// Computes statistics for this session.
    pub fn stats(&self) -> SessionStats {
        let total_groups = self.groups.len();
        let total_tabs = self.total_tab_count();

        let earliest_group = self.groups.iter().map(|g| g.created_at).min();
        let latest_group = self.groups.iter().map(|g| g.created_at).max();

        // Count tabs per domain
        let mut domain_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for group in &self.groups {
            for tab in &group.tabs {
                if let Some(domain) = tab.domain() {
                    *domain_counts.entry(domain.to_string()).or_insert(0) += 1;
                }
            }
        }

        // Sort by count descending and take top 10
        let mut domain_vec: Vec<_> = domain_counts.into_iter().collect();
        domain_vec.sort_by(|a, b| b.1.cmp(&a.1));
        let top_domains: Vec<_> = domain_vec.into_iter().take(10).collect();

        SessionStats {
            total_groups,
            total_tabs,
            earliest_group,
            latest_group,
            top_domains,
        }
    }

    /// Merges multiple sessions into a single session.
    ///
    /// The merged session will have:
    /// - version: highest version from input sessions
    /// - source: Unknown (since it's a merge)
    /// - groups: all groups from all sessions
    /// - created_at: earliest created_at from input sessions
    /// - imported_at: current time
    pub fn merge(sessions: Vec<TabSession>) -> TabSession {
        if sessions.is_empty() {
            return TabSession {
                version: 0,
                source: SessionSource::Unknown,
                groups: Vec::new(),
                created_at: Utc::now(),
                imported_at: Utc::now(),
            };
        }

        let version = sessions.iter().map(|s| s.version).max().unwrap_or(0);
        let all_groups: Vec<_> = sessions.iter().flat_map(|s| s.groups.clone()).collect();
        let created_at = sessions
            .iter()
            .map(|s| s.created_at)
            .min()
            .unwrap_or_else(|| Utc::now());

        TabSession {
            version,
            source: SessionSource::Unknown,
            groups: all_groups,
            created_at,
            imported_at: Utc::now(),
        }
    }
}

/// Error types for tablitz operations.
#[derive(Error, Debug)]
pub enum TablitzError {
    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Invalid URL
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    /// LevelDB error
    #[error("LevelDB error: {0}")]
    LevelDbError(String),

    /// Store error
    #[error("Store error: {0}")]
    StoreError(String),

    /// Other error
    #[error("Error: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, TablitzError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ms_timestamp_to_datetime() {
        // OneTab timestamp: 1760074389851
        let ms = 1760074389851;
        let dt = ms_timestamp_to_datetime(ms);
        assert!(dt.timestamp_millis() >= ms - 1000);
        assert!(dt.timestamp_millis() <= ms + 1000);
    }

    #[test]
    fn test_tab_domain() {
        let tab = Tab {
            id: "test".to_string(),
            url: Url::parse("https://example.com/path").unwrap(),
            title: "Test".to_string(),
            favicon_url: None,
            added_at: Utc::now(),
        };
        assert_eq!(tab.domain(), Some("example.com"));
    }

    #[test]
    fn test_tab_group_tab_count() {
        let group = TabGroup {
            id: "test".to_string(),
            label: Some("Test Group".to_string()),
            created_at: Utc::now(),
            tabs: vec![
                Tab {
                    id: "1".to_string(),
                    url: Url::parse("https://example.com").unwrap(),
                    title: "Tab 1".to_string(),
                    favicon_url: None,
                    added_at: Utc::now(),
                },
                Tab {
                    id: "2".to_string(),
                    url: Url::parse("https://example.org").unwrap(),
                    title: "Tab 2".to_string(),
                    favicon_url: None,
                    added_at: Utc::now(),
                },
            ],
            pinned: false,
            locked: false,
            starred: false,
        };
        assert_eq!(group.tab_count(), 2);
    }

    #[test]
    fn test_session_stats() {
        let session = TabSession {
            version: 1,
            source: SessionSource::Chrome {
                profile: "Default".to_string(),
            },
            groups: vec![TabGroup {
                id: "g1".to_string(),
                label: Some("Group 1".to_string()),
                created_at: Utc::now(),
                tabs: vec![
                    Tab {
                        id: "1".to_string(),
                        url: Url::parse("https://example.com").unwrap(),
                        title: "Tab 1".to_string(),
                        favicon_url: None,
                        added_at: Utc::now(),
                    },
                    Tab {
                        id: "2".to_string(),
                        url: Url::parse("https://example.com/other").unwrap(),
                        title: "Tab 2".to_string(),
                        favicon_url: None,
                        added_at: Utc::now(),
                    },
                ],
                pinned: false,
                locked: false,
                starred: false,
            }],
            created_at: Utc::now(),
            imported_at: Utc::now(),
        };

        let stats = session.stats();
        assert_eq!(stats.total_groups, 1);
        assert_eq!(stats.total_tabs, 2);
        assert_eq!(stats.top_domains.len(), 1);
        assert_eq!(stats.top_domains[0], ("example.com".to_string(), 2));
    }

    #[test]
    fn test_session_merge() {
        let session1 = TabSession {
            version: 1,
            source: SessionSource::Chrome {
                profile: "Default".to_string(),
            },
            groups: vec![TabGroup {
                id: "g1".to_string(),
                label: None,
                created_at: Utc::now(),
                tabs: vec![],
                pinned: false,
                locked: false,
                starred: false,
            }],
            created_at: Utc::now(),
            imported_at: Utc::now(),
        };

        let session2 = TabSession {
            version: 2,
            source: SessionSource::Edge {
                profile: "Default".to_string(),
            },
            groups: vec![TabGroup {
                id: "g2".to_string(),
                label: None,
                created_at: Utc::now(),
                tabs: vec![],
                pinned: false,
                locked: false,
                starred: false,
            }],
            created_at: Utc::now(),
            imported_at: Utc::now(),
        };

        let merged = TabSession::merge(vec![session1, session2]);
        assert_eq!(merged.version, 2);
        assert_eq!(merged.groups.len(), 2);
        assert!(matches!(merged.source, SessionSource::Unknown));
    }
}
