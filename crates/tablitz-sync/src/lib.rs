//! Git-backed snapshot and restore for the tablitz store.

use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use chrono::Utc;

/// Manages git-backed snapshots of the tablitz store.
pub struct SyncManager {
    repo_path: PathBuf,
    snapshot_filename: String,
}

impl SyncManager {
    pub fn new(repo_path: impl AsRef<Path>) -> Self {
        Self {
            repo_path: repo_path.as_ref().to_path_buf(),
            snapshot_filename: "tablitz-snapshot.json".to_string(),
        }
    }

    pub fn with_filename(repo_path: impl AsRef<Path>, filename: impl Into<String>) -> Self {
        Self {
            repo_path: repo_path.as_ref().to_path_buf(),
            snapshot_filename: filename.into(),
        }
    }

    pub fn snapshot_path(&self) -> PathBuf {
        self.repo_path.join(&self.snapshot_filename)
    }

    pub async fn snapshot(&self, store: &tablitz_store::Store) -> Result<String> {
        let session = store.get_session().await
            .context("failed to read store for snapshot")?;
        let json = serde_json::to_string_pretty(&session)
            .context("failed to serialize session")?;

        let snapshot_path = self.snapshot_path();
        std::fs::write(&snapshot_path, &json)
            .with_context(|| format!("failed to write snapshot to {}", snapshot_path.display()))?;

        let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
        let commit_msg = format!(
            "tablitz snapshot: {} groups, {} tabs ({})",
            session.groups.len(),
            session.total_tab_count(),
            timestamp
        );

        self.git(&["add", &self.snapshot_filename])
            .context("git add failed")?;
        let commit_output = self.git(&["commit", "-m", &commit_msg])
            .context("git commit failed")?;

        // git commit output looks like:
        //   "[main abc1234] message"       (subsequent commits)
        //   "[main (root-commit) abc1234] message"  (first commit)
        // Take the last token before ']'
        let hash = commit_output
            .lines()
            .next()
            .and_then(|line| {
                let bracket_content = line.trim_start_matches('[').split(']').next()?;
                bracket_content.split_whitespace().last().map(str::to_string)
            })
            .unwrap_or_else(|| "unknown".to_string());

        Ok(hash)
    }

    pub async fn restore(&self, store: &tablitz_store::Store) -> Result<(usize, usize)> {
        let snapshot_path = self.snapshot_path();
        let json = std::fs::read_to_string(&snapshot_path)
            .with_context(|| format!("failed to read snapshot from {}", snapshot_path.display()))?;

        let session: tablitz_core::TabSession = serde_json::from_str(&json)
            .context("failed to deserialize snapshot")?;

        let stats = store.insert_session(&session).await
            .context("failed to import snapshot into store")?;

        Ok((stats.groups_inserted, stats.tabs_inserted))
    }

    pub fn list_snapshots(&self, limit: usize) -> Result<Vec<SnapshotEntry>> {
        let output = self.git(&[
            "log", "--oneline",
            &format!("-{}", limit),
            "--", &self.snapshot_filename,
        ]).context("git log failed")?;

        let entries = output.lines()
            .filter(|l| !l.is_empty())
            .map(|line| {
                let mut parts = line.splitn(2, ' ');
                let hash = parts.next().unwrap_or("").to_string();
                let message = parts.next().unwrap_or("").to_string();
                SnapshotEntry { hash, message }
            })
            .collect();

        Ok(entries)
    }

    pub async fn restore_from_commit(
        &self,
        store: &tablitz_store::Store,
        commit_hash: &str,
    ) -> Result<(usize, usize)> {
        let output = self.git(&[
            "show",
            &format!("{}:{}", commit_hash, self.snapshot_filename),
        ]).context("git show failed")?;

        let session: tablitz_core::TabSession = serde_json::from_str(&output)
            .context("failed to deserialize snapshot from commit")?;

        let stats = store.insert_session(&session).await
            .context("failed to import snapshot into store")?;

        Ok((stats.groups_inserted, stats.tabs_inserted))
    }

    pub fn init_repo(&self) -> Result<()> {
        std::fs::create_dir_all(&self.repo_path)
            .with_context(|| format!("failed to create repo dir {}", self.repo_path.display()))?;
        self.git(&["init"]).context("git init failed")?;
        Ok(())
    }

    fn git(&self, args: &[&str]) -> Result<String> {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(&self.repo_path)
            .output()
            .context("failed to run git")?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git {:?} failed: {}", args, stderr)
        }
    }
}

#[derive(Debug, Clone)]
pub struct SnapshotEntry {
    pub hash: String,
    pub message: String,
}
