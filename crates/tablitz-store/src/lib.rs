//! libSQL-based storage for tablitz.
//!
//! This module provides persistent storage for tab groups and tabs using libSQL,
//! an embedded SQLite-compatible database.

use std::path::{Path, PathBuf};
use anyhow::Context;
use chrono::{DateTime, TimeZone, Utc};
use libsql::Builder;
use tablitz_core::{Tab, TabGroup, TabSession, SessionSource};
use url::Url;

/// Returns the default data directory for tablitz.
///
/// On Linux: `~/.local/share/tablitz`
/// On macOS: `~/Library/Application Support/tablitz`
/// On Windows: `%LOCALAPPDATA%\tablitz`
pub fn default_data_dir() -> anyhow::Result<PathBuf> {
    let dir = dirs::data_local_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot find data dir"))?
        .join("tablitz");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create data directory: {}", dir.display()))?;
    Ok(dir)
}

/// Statistics from a session insertion operation.
#[derive(Debug, Clone, Default)]
pub struct InsertStats {
    pub groups_inserted: usize,
    pub groups_skipped: usize,
    pub tabs_inserted: usize,
    pub tabs_skipped: usize,
}

/// Statistics about the store.
#[derive(Debug, Clone)]
pub struct StoreStats {
    pub total_groups: u64,
    pub total_tabs: u64,
    pub oldest_group: Option<DateTime<Utc>>,
    pub newest_group: Option<DateTime<Utc>>,
    pub top_domains: Vec<(String, u64)>,
}

/// libSQL-based storage for tablitz.
pub struct Store {
    conn: libsql::Connection,
}

impl Store {
    /// Opens a database at the specified path, initializing the schema if needed.
    pub async fn open(path: &Path) -> anyhow::Result<Self> {
        let db = Builder::new_local(path)
            .build()
            .await
            .with_context(|| format!("failed to open database at: {}", path.display()))?;
        let conn = db
            .connect()
            .with_context(|| "failed to get database connection")?;
        
        let store = Self { conn };
        store.init_schema().await?;
        Ok(store)
    }

    /// Opens the database at the default location.
    pub async fn open_default() -> anyhow::Result<Self> {
        let data_dir = default_data_dir()?;
        let db_path = data_dir.join("tablitz.db");
        Self::open(&db_path).await
    }

    /// Initializes the database schema if tables don't exist.
    async fn init_schema(&self) -> anyhow::Result<()> {
        // tab_groups table
        self.conn
            .execute(
                "CREATE TABLE IF NOT EXISTS tab_groups (
                    id TEXT PRIMARY KEY,
                    label TEXT,
                    created_at INTEGER NOT NULL,
                    pinned INTEGER NOT NULL DEFAULT 0,
                    locked INTEGER NOT NULL DEFAULT 0,
                    starred INTEGER NOT NULL DEFAULT 0,
                    source_type TEXT NOT NULL,
                    source_profile TEXT,
                    source_path TEXT
                )",
                (),
            )
            .await
            .context("failed to create tab_groups table")?;

        // tabs table
        self.conn
            .execute(
                "CREATE TABLE IF NOT EXISTS tabs (
                    id TEXT PRIMARY KEY,
                    group_id TEXT NOT NULL REFERENCES tab_groups(id) ON DELETE CASCADE,
                    url TEXT NOT NULL,
                    title TEXT NOT NULL,
                    favicon_url TEXT,
                    added_at INTEGER NOT NULL,
                    position INTEGER NOT NULL DEFAULT 0
                )",
                (),
            )
            .await
            .context("failed to create tabs table")?;

        // indexes
        self.conn
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_tabs_group_id ON tabs(group_id)",
                (),
            )
            .await
            .context("failed to create idx_tabs_group_id")?;

        self.conn
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_tabs_url ON tabs(url)",
                (),
            )
            .await
            .context("failed to create idx_tabs_url")?;

        self.conn
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_tab_groups_created_at ON tab_groups(created_at)",
                (),
            )
            .await
            .context("failed to create idx_tab_groups_created_at")?;

        Ok(())
    }

    /// Inserts a complete session into the database.
    ///
    /// Groups from the same source are deduplicated (checked by id).
    /// Returns statistics about how many groups/tabs were inserted vs skipped.
    pub async fn insert_session(&self, session: &TabSession) -> anyhow::Result<InsertStats> {
        let tx = self
            .conn
            .transaction()
            .await
            .context("failed to start transaction")?;

        let mut stats = InsertStats::default();
        let source_type = session_source_type_to_string(&session.source);
        let source_profile = session_source_profile_to_string(&session.source);
        let source_path = session_source_path_to_string(&session.source);

        for group in &session.groups {
            let group_inserted = match tx
                .execute(
                    "INSERT OR IGNORE INTO tab_groups 
                        (id, label, created_at, pinned, locked, starred, source_type, source_profile, source_path)
                        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    libsql::params![
                        group.id.clone(),
                        group.label.clone(),
                        group.created_at.timestamp_millis(),
                        group.pinned as i64,
                        group.locked as i64,
                        group.starred as i64,
                        source_type.clone(),
                        source_profile.as_deref(),
                        source_path.as_deref(),
                    ],
                )
                .await
            {
                Ok(rows_affected) if rows_affected > 0 => true,
                _ => false,
            };

            if group_inserted {
                stats.groups_inserted += 1;
                // Insert tabs for this group
                for (position, tab) in group.tabs.iter().enumerate() {
                    match tx
                        .execute(
                            "INSERT OR IGNORE INTO tabs 
                                (id, group_id, url, title, favicon_url, added_at, position)
                                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                            libsql::params![
                                tab.id.clone(),
                                group.id.clone(),
                                tab.url.as_str(),
                                tab.title.clone(),
                                tab.favicon_url.as_deref(),
                                tab.added_at.timestamp_millis(),
                                position as i64,
                            ],
                        )
                        .await
                    {
                        Ok(rows_affected) if rows_affected > 0 => {
                            stats.tabs_inserted += 1;
                        }
                        _ => {
                            stats.tabs_skipped += 1;
                        }
                    }
                }
            } else {
                stats.groups_skipped += 1;
                // Tabs for this group already exist
                let tab_count = group.tabs.len();
                stats.tabs_skipped += tab_count;
            }
        }

        tx.commit().await.context("failed to commit transaction")?;
        Ok(stats)
    }

    /// Inserts a single tab group into the database.
    pub async fn insert_group(&self, group: &TabGroup) -> anyhow::Result<()> {
        self.conn
            .execute(
                "INSERT INTO tab_groups 
                    (id, label, created_at, pinned, locked, starred, source_type, source_profile, source_path)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                libsql::params![
                    group.id.clone(),
                    group.label.clone(),
                    group.created_at.timestamp_millis(),
                    group.pinned as i64,
                    group.locked as i64,
                    group.starred as i64,
                    "manual",  // source_type for manually inserted groups
                    None::<&str>,
                    None::<&str>,
                ],
            )
            .await
            .context("failed to insert tab_group")?;

        // Insert tabs for this group
        for (position, tab) in group.tabs.iter().enumerate() {
            self.conn
                .execute(
                    "INSERT OR IGNORE INTO tabs 
                        (id, group_id, url, title, favicon_url, added_at, position)
                        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    libsql::params![
                        tab.id.clone(),
                        group.id.clone(),
                        tab.url.as_str(),
                        tab.title.clone(),
                        tab.favicon_url.as_deref(),
                        tab.added_at.timestamp_millis(),
                        position as i64,
                    ],
                )
                .await
                .context("failed to insert tab")?;
        }

        Ok(())
    }

    /// Deletes a tab group and all its tabs.
    pub async fn delete_group(&self, group_id: &str) -> anyhow::Result<()> {
        self.conn
            .execute("DELETE FROM tab_groups WHERE id = ?1", libsql::params![group_id])
            .await
            .context("failed to delete tab_group")?;
        Ok(())
    }

    /// Returns all tab groups with their tabs.
    pub async fn get_all_groups(&self) -> anyhow::Result<Vec<TabGroup>> {
        let mut group_rows = self
            .conn
            .query("SELECT id, label, created_at, pinned, locked, starred, source_type, source_profile, source_path FROM tab_groups ORDER BY created_at DESC", ())
            .await
            .context("failed to query tab_groups")?;

        let mut groups = Vec::new();

        while let Ok(Some(row)) = group_rows.next().await {
            let row: libsql::Row = row;  // row is already unwrapped from Ok()
            let group_id: String = row.get(0)?;
            let label: Option<String> = row.get(1)?;
            let created_at_ms: i64 = row.get(2)?;
            let pinned: i64 = row.get(3)?;
            let locked: i64 = row.get(4)?;
            let starred: i64 = row.get(5)?;

            let created_at = Utc.timestamp_millis_opt(created_at_ms)
                .single()
                .unwrap_or_else(|| Utc::now());

            // Fetch tabs for this group
            let tabs = self.get_tabs_for_group(&group_id).await?;

            groups.push(TabGroup {
                id: group_id,
                label,
                created_at,
                tabs,
                pinned: pinned != 0,
                locked: locked != 0,
                starred: starred != 0,
            });
        }

        Ok(groups)
    }

    /// Returns all tabs for a specific group.
    pub async fn get_tabs_for_group(&self, group_id: &str) -> anyhow::Result<Vec<Tab>> {
        let mut tab_rows = self
            .conn
            .query(
                "SELECT id, url, title, favicon_url, added_at FROM tabs WHERE group_id = ?1 ORDER BY position",
                libsql::params![group_id],
            )
            .await
            .context("failed to query tabs")?;

        let mut tabs = Vec::new();

        while let Ok(Some(row)) = tab_rows.next().await {
            let row: libsql::Row = row;  // row is already unwrapped from Ok()
            tabs.push(row_to_tab(row)?);
        }

        Ok(tabs)
    }

    /// Returns a complete session (all groups and tabs).
    pub async fn get_session(&self) -> anyhow::Result<TabSession> {
        let groups = self.get_all_groups().await?;

        Ok(TabSession {
            version: 1,
            source: SessionSource::Unknown,
            groups,
            created_at: Utc::now(),
            imported_at: Utc::now(),
        })
    }

    /// Searches for tabs by URL (partial match).
    pub async fn search_by_url(&self, query: &str) -> anyhow::Result<Vec<Tab>> {
        let pattern = format!("%{}%", query);
        let mut tab_rows = self
            .conn
            .query(
                "SELECT id, url, title, favicon_url, added_at FROM tabs WHERE url LIKE ?1",
                libsql::params![pattern.clone()],
            )
            .await
            .context("failed to search tabs by url")?;

        let mut tabs = Vec::new();

        while let Some(row) = tab_rows.next().await? {
            tabs.push(row_to_tab(row)?);
        }

        Ok(tabs)
    }

    /// Searches for tabs by title (partial match).
    pub async fn search_by_title(&self, query: &str) -> anyhow::Result<Vec<Tab>> {
        let pattern = format!("%{}%", query);
        let mut tab_rows = self
            .conn
            .query(
                "SELECT id, url, title, favicon_url, added_at FROM tabs WHERE title LIKE ?1",
                libsql::params![pattern.clone()],
            )
            .await
            .context("failed to search tabs by title")?;

        let mut tabs = Vec::new();

        while let Some(row) = tab_rows.next().await? {
            tabs.push(row_to_tab(row)?);
        }

        Ok(tabs)
    }

    /// Returns store statistics.
    pub async fn get_stats(&self) -> anyhow::Result<StoreStats> {
        // Count groups
        let mut count_rows = self
            .conn
            .query("SELECT COUNT(*) as count FROM tab_groups", ())
            .await?;
        let count_row = count_rows.next().await?
            .ok_or_else(|| anyhow::anyhow!("no result from COUNT groups"))?;
        let total_groups: u64 = count_row.get(0)?;

        // Count tabs
        let mut count_rows = self
            .conn
            .query("SELECT COUNT(*) as count FROM tabs", ())
            .await?;
        let count_row = count_rows.next().await?
            .ok_or_else(|| anyhow::anyhow!("no result from COUNT tabs"))?;
        let total_tabs: u64 = count_row.get(0)?;

        // Oldest and newest group
        let oldest_group = {
            let mut rows = self
                .conn
                .query("SELECT MIN(created_at) as min FROM tab_groups", ())
                .await?;
            if let Ok(Some(row)) = rows.next().await {
                let row: libsql::Row = row;
                let ms: Option<i64> = row.get(0)?;
                ms.map(|m| Utc.timestamp_millis_opt(m).single().unwrap_or_else(|| Utc::now()))
            } else {
                None
            }
        };

        let newest_group = {
            let mut rows = self
                .conn
                .query("SELECT MAX(created_at) as max FROM tab_groups", ())
                .await?;
            if let Ok(Some(row)) = rows.next().await {
                let row: libsql::Row = row;
                let ms: Option<i64> = row.get(0)?;
                ms.map(|m| Utc.timestamp_millis_opt(m).single().unwrap_or_else(|| Utc::now()))
            } else {
                None
            }
        };

        // Top domains
        let mut domain_counts = std::collections::HashMap::new();
        let mut tab_rows = self
            .conn
            .query("SELECT url FROM tabs", ())
            .await
            .context("failed to query tabs for domain stats")?;

        while let Ok(Some(row)) = tab_rows.next().await {
            let row: libsql::Row = row;  // row is already unwrapped from Ok()
            let url_str: String = row.get(0)?;
            if let Some(host) = extract_host(&url_str) {
                *domain_counts.entry(host).or_insert(0u64) += 1;
            }
        }

        let mut top_domains: Vec<_> = domain_counts.into_iter().collect();
        top_domains.sort_by(|a, b| b.1.cmp(&a.1));
        top_domains.truncate(10);

        Ok(StoreStats {
            total_groups,
            total_tabs,
            oldest_group,
            newest_group,
            top_domains,
        })
    }
}

/// Extracts the source type string from SessionSource.
fn session_source_type_to_string(source: &SessionSource) -> String {
    match source {
        SessionSource::Chrome { .. } => "Chrome".to_string(),
        SessionSource::Edge { .. } => "Edge".to_string(),
        SessionSource::Brave { .. } => "Brave".to_string(),
        SessionSource::Comet { .. } => "Comet".to_string(),
        SessionSource::OneTabExport { .. } => "OneTabExport".to_string(),
        SessionSource::TablitzNative { .. } => "TablitzNative".to_string(),
        SessionSource::Unknown => "Unknown".to_string(),
    }
}

/// Extracts the profile string from SessionSource, if applicable.
fn session_source_profile_to_string(source: &SessionSource) -> Option<String> {
    match source {
        SessionSource::Chrome { profile } => Some(profile.clone()),
        SessionSource::Edge { profile } => Some(profile.clone()),
        SessionSource::Brave { profile } => Some(profile.clone()),
        SessionSource::Comet { profile } => Some(profile.clone()),
        SessionSource::OneTabExport { .. } => None,
        SessionSource::TablitzNative { .. } => None,
        SessionSource::Unknown => None,
    }
}

/// Extracts the path string from SessionSource, if applicable.
fn session_source_path_to_string(source: &SessionSource) -> Option<String> {
    match source {
        SessionSource::Chrome { .. } => None,
        SessionSource::Edge { .. } => None,
        SessionSource::Brave { .. } => None,
        SessionSource::Comet { .. } => None,
        SessionSource::OneTabExport { path } => Some(path.clone()),
        SessionSource::TablitzNative { path } => Some(path.clone()),
        SessionSource::Unknown => None,
    }
}

/// Converts a database row to a Tab.
fn row_to_tab(row: libsql::Row) -> anyhow::Result<Tab> {
    let id: String = row.get(0)?;
    let url_str: String = row.get(1)?;
    let title: String = row.get(2)?;
    let favicon_url: Option<String> = row.get(3)?;
    let added_at_ms: i64 = row.get(4)?;

    let url = Url::parse(&url_str)
        .with_context(|| format!("invalid URL in database: {}", url_str))?;

    let added_at = Utc.timestamp_millis_opt(added_at_ms)
        .single()
        .unwrap_or_else(|| Utc::now());

    Ok(Tab {
        id,
        url,
        title,
        favicon_url,
        added_at,
    })
}

/// Extracts the hostname from a URL string (no external dependencies).
fn extract_host(url: &str) -> Option<String> {
    let after_scheme = url.split("://").nth(1)?;
    let host = after_scheme.split('/').next()?;
    let host = host.split('@').last()?;
    let host = host.split(':').next()?;
    Some(host.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_default_data_dir() {
        let dir = default_data_dir().unwrap();
        assert!(dir.exists());
        assert!(dir.ends_with("tablitz"));
    }

    #[test]
    fn test_extract_host() {
        assert_eq!(
            extract_host("https://example.com/path"),
            Some("example.com".to_string())
        );
        assert_eq!(
            extract_host("http://example.org"),
            Some("example.org".to_string())
        );
        assert_eq!(
            extract_host("https://user:pass@EXAMPLE.COM:8080/path"),
            Some("example.com".to_string())
        );
        assert_eq!(extract_host("about:blank"), None);
        assert_eq!(extract_host("invalid-url"), None);
    }

    #[tokio::test]
    async fn test_store_roundtrip() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let store = Store::open(&db_path).await.unwrap();

        let session = TabSession {
            version: 1,
            source: SessionSource::Chrome {
                profile: "Default".to_string(),
            },
            groups: vec![TabGroup {
                id: "group1".to_string(),
                label: Some("Test Group".to_string()),
                created_at: Utc::now(),
                tabs: vec![Tab {
                    id: "tab1".to_string(),
                    url: Url::parse("https://example.com").unwrap(),
                    title: "Example".to_string(),
                    favicon_url: None,
                    added_at: Utc::now(),
                }],
                pinned: false,
                locked: false,
                starred: true,
            }],
            created_at: Utc::now(),
            imported_at: Utc::now(),
        };

        let stats = store.insert_session(&session).await.unwrap();
        assert_eq!(stats.groups_inserted, 1);
        assert_eq!(stats.tabs_inserted, 1);

        let groups = store.get_all_groups().await.unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].id, "group1");
        assert_eq!(groups[0].label, Some("Test Group".to_string()));
        assert_eq!(groups[0].tabs.len(), 1);
        assert_eq!(groups[0].tabs[0].id, "tab1");
        assert_eq!(groups[0].starred, true);
    }

    #[tokio::test]
    async fn test_delete_group() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let store = Store::open(&db_path).await.unwrap();

        let group = TabGroup {
            id: "group1".to_string(),
            label: None,
            created_at: Utc::now(),
            tabs: vec![Tab {
                id: "tab1".to_string(),
                url: Url::parse("https://example.com").unwrap(),
                title: "Example".to_string(),
                favicon_url: None,
                added_at: Utc::now(),
            }],
            pinned: false,
            locked: false,
            starred: false,
        };

        store.insert_group(&group).await.unwrap();
        assert_eq!(store.get_all_groups().await.unwrap().len(), 1);

        store.delete_group("group1").await.unwrap();
        assert_eq!(store.get_all_groups().await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_search() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let store = Store::open(&db_path).await.unwrap();

        let group = TabGroup {
            id: "group1".to_string(),
            label: None,
            created_at: Utc::now(),
            tabs: vec![
                Tab {
                    id: "tab1".to_string(),
                    url: Url::parse("https://example.com/path").unwrap(),
                    title: "Example Page".to_string(),
                    favicon_url: None,
                    added_at: Utc::now(),
                },
                Tab {
                    id: "tab2".to_string(),
                    url: Url::parse("https://other.org").unwrap(),
                    title: "Another Page".to_string(),
                    favicon_url: None,
                    added_at: Utc::now(),
                },
            ],
            pinned: false,
            locked: false,
            starred: false,
        };

        store.insert_group(&group).await.unwrap();

        let url_results = store.search_by_url("example").await.unwrap();
        assert_eq!(url_results.len(), 1);
        assert_eq!(url_results[0].id, "tab1");

        let title_results = store.search_by_title("Page").await.unwrap();
        assert_eq!(title_results.len(), 2);
    }

    #[tokio::test]
    async fn test_stats() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let store = Store::open(&db_path).await.unwrap();

        let session = TabSession {
            version: 1,
            source: SessionSource::Unknown,
            groups: vec![
                TabGroup {
                    id: "group1".to_string(),
                    label: None,
                    created_at: Utc::now(),
                    tabs: vec![
                        Tab {
                            id: "tab1".to_string(),
                            url: Url::parse("https://example.com").unwrap(),
                            title: "Example".to_string(),
                            favicon_url: None,
                            added_at: Utc::now(),
                        },
                        Tab {
                            id: "tab2".to_string(),
                            url: Url::parse("https://example.com").unwrap(),
                            title: "Example 2".to_string(),
                            favicon_url: None,
                            added_at: Utc::now(),
                        },
                    ],
                    pinned: false,
                    locked: false,
                    starred: false,
                },
                TabGroup {
                    id: "group2".to_string(),
                    label: None,
                    created_at: Utc::now(),
                    tabs: vec![Tab {
                        id: "tab3".to_string(),
                        url: Url::parse("https://other.org").unwrap(),
                        title: "Other".to_string(),
                        favicon_url: None,
                        added_at: Utc::now(),
                    }],
                    pinned: false,
                    locked: false,
                    starred: false,
                },
            ],
            created_at: Utc::now(),
            imported_at: Utc::now(),
        };

        store.insert_session(&session).await.unwrap();

        let stats = store.get_stats().await.unwrap();
        assert_eq!(stats.total_groups, 2);
        assert_eq!(stats.total_tabs, 3);
        assert_eq!(stats.top_domains[0], ("example.com".to_string(), 2));
    }
}
