use std::path::PathBuf;
use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use colored::Colorize;

#[derive(Parser)]
#[command(name = "tablitz", version, about = "Recover, manage, search, and back up your OneTab data")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Recover OneTab data from a browser LevelDB store
    Recover {
        #[arg(long, value_enum, default_value = "chrome")]
        browser: BrowserArg,
        #[arg(long, default_value = "Default")]
        profile: String,
        #[arg(long, value_enum, default_value = "json")]
        format: OutputFormat,
        #[arg(long, short)]
        out: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        db_path: Option<PathBuf>,
    },
    /// Import tab data into the tablitz store
    Import {
        #[arg(long)]
        from_onetab_export: Option<PathBuf>,
        #[arg(long)]
        from_onetab_leveldb: Option<PathBuf>,
        #[arg(long, value_enum)]
        browser: Option<BrowserArg>,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        from_tablitz: Option<PathBuf>,
    },
    /// Export tab data from the store
    Export {
        #[arg(long, value_enum, default_value = "markdown")]
        format: ExportFormat,
        #[arg(long, short)]
        out: Option<PathBuf>,
        #[arg(long)]
        filter: Option<String>,
    },
    /// Search the tablitz store
    Search {
        query: String,
        #[arg(long, value_enum, default_value = "fuzzy")]
        mode: SearchMode,
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// List tab groups
    List {
        #[arg(long)]
        filter: Option<String>,
        #[arg(long, default_value = "50")]
        limit: usize,
    },
    /// Deduplicate and normalize tab data
    Dedup {
        #[arg(long, value_enum, default_value = "normalized-url")]
        strategy: DedupStrategyArg,
        #[arg(long)]
        normalize_titles: bool,
        #[arg(long)]
        dry_run: bool,
    },
    /// Initialize tablitz (create config directory)
    Init,
    /// Show store statistics
    Stats,
    /// Start MCP server (requires --features mcp)
    Serve {
        #[arg(long, default_value = "0")]
        port: u16,
    },
    /// Snapshot the store to a git-backed repo
    Snapshot {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        filename: Option<String>,
    },
    /// Restore the store from a git-backed snapshot
    Restore {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        commit: Option<String>,
        #[arg(long)]
        filename: Option<String>,
    },
    /// List recent snapshots in a git-backed repo
    Snapshots {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long, default_value = "10")]
        limit: usize,
    },
}

#[derive(ValueEnum, Clone, Debug)]
enum BrowserArg {
    Chrome,
    Edge,
    Brave,
    Comet,
}

#[derive(ValueEnum, Clone, Debug)]
enum OutputFormat {
    Json,
    Markdown,
    Table,
    Toml,
}

#[derive(ValueEnum, Clone, Debug)]
enum ExportFormat {
    Json,
    Markdown,
    Toml,
}

#[derive(ValueEnum, Clone, Debug)]
enum SearchMode {
    Fuzzy,
    FullText,
}

#[derive(ValueEnum, Clone, Debug)]
enum DedupStrategyArg {
    ExactUrl,
    NormalizedUrl,
    UrlAndTitle,
}

fn browser_arg_to_recover(b: &BrowserArg) -> tablitz_recover::Browser {
    match b {
        BrowserArg::Chrome => tablitz_recover::Browser::Chrome,
        BrowserArg::Edge => tablitz_recover::Browser::Edge,
        BrowserArg::Brave => tablitz_recover::Browser::Brave,
        BrowserArg::Comet => tablitz_recover::Browser::Comet,
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(run(cli))
}

async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Recover { browser, profile, dry_run, db_path, out, format } => {
            cmd_recover(browser, profile, dry_run, db_path, out, format).await
        }
        Commands::Import { from_onetab_export, from_onetab_leveldb, browser, profile, from_tablitz } => {
            cmd_import(from_onetab_export, from_onetab_leveldb, browser, profile, from_tablitz).await
        }
        Commands::Export { format, out, filter } => {
            cmd_export(format, out, filter).await
        }
        Commands::Search { query, mode, limit } => {
            cmd_search(query, mode, limit).await
        }
        Commands::List { filter, limit } => {
            cmd_list(filter, limit).await
        }
        Commands::Dedup { strategy, normalize_titles, dry_run } => {
            cmd_dedup(strategy, normalize_titles, dry_run).await
        }
        Commands::Init => cmd_init().await,
        Commands::Stats => cmd_stats().await,
        Commands::Serve { port: _ } => cmd_serve().await,
        Commands::Snapshot { repo, filename } => cmd_snapshot(repo, filename).await,
        Commands::Restore { repo, commit, filename } => cmd_restore(repo, commit, filename).await,
        Commands::Snapshots { repo, limit } => cmd_snapshots(repo, limit).await,
    }
}

async fn cmd_recover(
    browser: BrowserArg,
    profile: String,
    dry_run: bool,
    db_path: Option<PathBuf>,
    out: Option<PathBuf>,
    _format: OutputFormat,
) -> Result<()> {
    use indicatif::{ProgressBar, ProgressStyle};
    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyle::default_spinner().template("{spinner} {msg}").unwrap());
    pb.set_message(format!("Recovering from {} profile '{}'...", format!("{:?}", browser).to_lowercase(), profile));
    pb.enable_steady_tick(std::time::Duration::from_millis(80));

    let opts = tablitz_recover::RecoverOptions {
        browser: browser_arg_to_recover(&browser),
        profile,
        dry_run,
        db_path,
    };
    let session = tablitz_recover::recover(opts)?;
    pb.finish_and_clear();

    let group_count = session.groups.len();
    let tab_count = session.total_tab_count();
    println!("{} Recovered {} groups, {} tabs", "✓".green(), group_count, tab_count);

    if dry_run {
        println!("{}", "(dry run — nothing imported)".dimmed());
        return Ok(());
    }

    if let Some(path) = out {
        let json = serde_json::to_string_pretty(&session)?;
        std::fs::write(&path, json)?;
        println!("  Saved to {}", path.display());
    } else {
        let store = tablitz_store::Store::open_default().await?;
        let stats = store.insert_session(&session).await?;
        println!(
            "  Imported: {} groups, {} tabs (skipped: {} groups, {} tabs)",
            stats.groups_inserted, stats.tabs_inserted,
            stats.groups_skipped, stats.tabs_skipped
        );
    }
    Ok(())
}

async fn cmd_import(
    from_onetab_export: Option<PathBuf>,
    from_onetab_leveldb: Option<PathBuf>,
    browser: Option<BrowserArg>,
    profile: Option<String>,
    _from_tablitz: Option<PathBuf>,
) -> Result<()> {
    let store = tablitz_store::Store::open_default().await?;

    if let Some(path) = from_onetab_export {
        println!("Importing from OneTab export: {}", path.display());
        let session = tablitz_recover::parse_onetab_export(&path)?;
        let stats = store.insert_session(&session).await?;
        println!(
            "{} {} groups inserted, {} skipped",
            "✓".green(), stats.groups_inserted, stats.groups_skipped
        );
        println!(
            "  {} tabs inserted, {} skipped",
            stats.tabs_inserted, stats.tabs_skipped
        );
    } else if let Some(path) = from_onetab_leveldb {
        let b = browser.unwrap_or(BrowserArg::Chrome);
        let p = profile.unwrap_or_else(|| "Default".to_string());
        let source = match &b {
            BrowserArg::Chrome => tablitz_core::SessionSource::Chrome { profile: p.clone() },
            BrowserArg::Edge => tablitz_core::SessionSource::Edge { profile: p.clone() },
            BrowserArg::Brave => tablitz_core::SessionSource::Brave { profile: p.clone() },
            BrowserArg::Comet => tablitz_core::SessionSource::Comet { profile: p.clone() },
        };
        let session = tablitz_recover::extract_from_leveldb(&path, source)?;
        let stats = store.insert_session(&session).await?;
        println!(
            "{} Imported from LevelDB ({}): {} groups, {} tabs",
            "✓".green(), format!("{:?}", b).to_lowercase(),
            stats.groups_inserted, stats.tabs_inserted
        );
    } else {
        eprintln!("{} No import source specified. Use --from-onetab-export or --from-onetab-leveldb", "✗".red());
        std::process::exit(1);
    }
    Ok(())
}

async fn cmd_export(format: ExportFormat, out: Option<PathBuf>, filter: Option<String>) -> Result<()> {
    let store = tablitz_store::Store::open_default().await?;
    let session = store.get_session().await?;

    let groups: Vec<_> = if let Some(ref f) = filter {
        session.groups.iter()
            .filter(|g| g.label.as_deref().unwrap_or("").contains(f.as_str()))
            .cloned()
            .collect()
    } else {
        session.groups.clone()
    };

    let content = match format {
        ExportFormat::Json => serde_json::to_string_pretty(&groups)?,
        ExportFormat::Toml => toml::to_string(&groups)?,
        ExportFormat::Markdown => {
            let mut md = String::new();
            for group in &groups {
                md.push_str(&format!("---\n## {} tabs\n", group.tabs.len()));
                md.push_str(&format!("> Created {}\n", group.created_at.format("%-m/%-d/%Y, %-I:%M:%S %p")));
                if let Some(label) = &group.label {
                    md.push_str(&format!("> {}\n", label));
                }
                md.push('\n');
                for tab in &group.tabs {
                    md.push_str(&format!("[{}]({})\n", tab.title, tab.url));
                }
                md.push('\n');
            }
            md
        }
    };

    if let Some(path) = out {
        std::fs::write(&path, &content)?;
        println!("{} Exported {} groups to {}", "✓".green(), groups.len(), path.display());
    } else {
        print!("{}", content);
    }
    Ok(())
}

async fn cmd_search(query: String, mode: SearchMode, limit: usize) -> Result<()> {
    let store = tablitz_store::Store::open_default().await?;
    match mode {
        SearchMode::Fuzzy => {
            let session = store.get_session().await?;
            let results = tablitz_search::FuzzySearcher::search(&query, &session);
            let results: Vec<_> = results.into_iter().take(limit).collect();
            if results.is_empty() {
                println!("No results for '{}'", query);
                return Ok(());
            }
            println!("{} results for '{}':", results.len(), query.bold());
            for r in &results {
                println!("  [{:.2}] {} \n        {}", r.score, r.tab.title.cyan(), r.tab.url.as_str().dimmed());
            }
        }
        SearchMode::FullText => {
            let by_url = store.search_by_url(&query).await?;
            let by_title = store.search_by_title(&query).await?;
            let mut seen = std::collections::HashSet::new();
            let merged: Vec<_> = by_url.into_iter().chain(by_title)
                .filter(|t| seen.insert(t.url.to_string()))
                .take(limit).collect();
            if merged.is_empty() {
                println!("No results for '{}'", query);
                return Ok(());
            }
            println!("{} results for '{}':", merged.len(), query.bold());
            for tab in &merged {
                println!("  {} \n        {}", tab.title.cyan(), tab.url.as_str().dimmed());
            }
        }
    }
    Ok(())
}

async fn cmd_list(filter: Option<String>, limit: usize) -> Result<()> {
    let store = tablitz_store::Store::open_default().await?;
    let groups = store.get_all_groups().await?;

    let groups: Vec<_> = if let Some(ref f) = filter {
        groups.into_iter()
            .filter(|g| g.label.as_deref().unwrap_or("").contains(f.as_str()))
            .collect()
    } else {
        groups
    };

    let groups: Vec<_> = groups.into_iter().take(limit).collect();

    if groups.is_empty() {
        println!("No groups found.");
        return Ok(());
    }

    println!("{} groups:", groups.len().to_string().bold());
    for group in &groups {
        let label = group.label.as_deref().unwrap_or("(unlabeled)");
        let flags = format!(
            "{}{}{}",
            if group.pinned { "📌" } else { "" },
            if group.locked { "🔒" } else { "" },
            if group.starred { "⭐" } else { "" },
        );
        println!(
            "  {} {} {} ({} tabs)",
            group.id.get(..8).unwrap_or(&group.id).dimmed(),
            label.cyan(),
            flags,
            group.tabs.len()
        );
    }
    Ok(())
}

async fn cmd_dedup(strategy: DedupStrategyArg, normalize_titles: bool, dry_run: bool) -> Result<()> {
    let store = tablitz_store::Store::open_default().await?;
    let session = store.get_session().await?;

    let mut working = session.clone();
    if normalize_titles {
        working = tablitz_search::TitleNormalizer::normalize_session(&working);
    }

    let dedup_strategy = match strategy {
        DedupStrategyArg::ExactUrl => tablitz_search::DedupStrategy::ExactUrl,
        DedupStrategyArg::NormalizedUrl => tablitz_search::DedupStrategy::NormalizedUrl,
        DedupStrategyArg::UrlAndTitle => tablitz_search::DedupStrategy::UrlAndTitle,
    };

    let result = tablitz_search::DedupEngine::dedup(&working, dedup_strategy);
    println!(
        "Dedup: {} → {} tabs ({} removed)",
        result.original_count, result.deduplicated_count,
        result.original_count - result.deduplicated_count
    );

    if dry_run {
        println!("{}", "(dry run — nothing saved)".dimmed());
        return Ok(());
    }

    let group_count = result.session.groups.len();
    for group in &result.session.groups {
        store.replace_tabs_for_group(group).await?;
    }
    println!("{} Persisted deduplicated tabs ({} groups updated)", "✓".green(), group_count);
    Ok(())
}

async fn cmd_init() -> Result<()> {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot find config dir"))?
        .join("tablitz");
    std::fs::create_dir_all(&config_dir)?;
    println!("{} Initialized tablitz at {}", "✓".green(), config_dir.display());

    let data_dir = tablitz_store::default_data_dir()?;
    println!("  Data dir: {}", data_dir.display());
    Ok(())
}

async fn cmd_stats() -> Result<()> {
    let store = tablitz_store::Store::open_default().await?;
    let stats = store.get_stats().await?;

    println!("{}", "tablitz store stats".bold());
    println!("  Groups: {}", stats.total_groups.to_string().cyan());
    println!("  Tabs:   {}", stats.total_tabs.to_string().cyan());

    if let Some(oldest) = stats.oldest_group {
        println!("  Oldest: {}", oldest.format("%Y-%m-%d").to_string().dimmed());
    }
    if let Some(newest) = stats.newest_group {
        println!("  Newest: {}", newest.format("%Y-%m-%d").to_string().dimmed());
    }

    if !stats.top_domains.is_empty() {
        println!("\n  Top domains:");
        for (domain, count) in stats.top_domains.iter().take(10) {
            println!("    {:40} {}", domain.cyan(), count);
        }
    }
    Ok(())
}

async fn cmd_snapshot(repo: PathBuf, filename: Option<String>) -> Result<()> {
    let store = tablitz_store::Store::open_default().await?;
    let mut mgr = tablitz_sync::SyncManager::new(&repo);
    if let Some(name) = filename {
        mgr = tablitz_sync::SyncManager::with_filename(&repo, name);
    }
    let hash = mgr.snapshot(&store).await?;
    println!("{} Snapshot committed: {}", "✓".green(), hash);
    Ok(())
}

async fn cmd_restore(repo: PathBuf, commit: Option<String>, filename: Option<String>) -> Result<()> {
    let store = tablitz_store::Store::open_default().await?;
    let mut mgr = tablitz_sync::SyncManager::new(&repo);
    if let Some(name) = filename {
        mgr = tablitz_sync::SyncManager::with_filename(&repo, name);
    }
    let (groups, tabs) = if let Some(hash) = commit {
        mgr.restore_from_commit(&store, &hash).await?
    } else {
        mgr.restore(&store).await?
    };
    println!("{} Restored: {} groups, {} tabs imported", "✓".green(), groups, tabs);
    Ok(())
}

async fn cmd_snapshots(repo: PathBuf, limit: usize) -> Result<()> {
    let mgr = tablitz_sync::SyncManager::new(&repo);
    let snapshots = mgr.list_snapshots(limit)?;
    if snapshots.is_empty() {
        println!("No snapshots found.");
        return Ok(());
    }
    for s in &snapshots {
        println!("  {} {}", s.hash.dimmed(), s.message);
    }
    Ok(())
}

#[cfg(feature = "mcp")]

#[cfg(feature = "mcp")]
async fn cmd_serve() -> Result<()> {
    use rmcp::{ServiceExt, transport::stdio};
    let store = tablitz_store::Store::open_default().await?;
    let server = mcp::TablitzMcpServer::new(store);
    println!("tablitz MCP server starting on stdio...");
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(not(feature = "mcp"))]
async fn cmd_serve() -> Result<()> {
    eprintln!("MCP server requires the 'mcp' feature. Rebuild with: cargo build --features mcp");
    std::process::exit(1);
}

#[cfg(feature = "mcp")]
mod mcp {
    use std::sync::Arc;
    use rmcp::{
        ErrorData as McpError,
        ServerHandler,
        model::{CallToolResult, Content},
        handler::server::tool::ToolRouter,
        handler::server::wrapper::Parameters,
        tool, tool_handler, tool_router,
    };
    use rmcp::schemars::JsonSchema;
    use serde::Deserialize;

    #[derive(Clone)]
    pub struct TablitzMcpServer {
        store: Arc<tablitz_store::Store>,
        tool_router: ToolRouter<Self>,
    }

    #[tool_router]
    impl TablitzMcpServer {
        pub fn new(store: tablitz_store::Store) -> Self {
            Self {
                store: Arc::new(store),
                tool_router: Self::tool_router(),
            }
        }

        #[tool(name = "search_tabs", description = "Search tabs by query using fuzzy matching")]
        async fn search_tabs(
            &self,
            Parameters(params): Parameters<SearchTabsParams>,
        ) -> Result<CallToolResult, McpError> {
            let limit = params.limit.unwrap_or(20);
            let session = self.store.get_session().await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;
            let results = tablitz_search::FuzzySearcher::search(&params.query, &session);
            let results: Vec<_> = results.into_iter().take(limit).collect();
            let text = results.iter().map(|r| {
                format!("[{:.2}] {}\n        {}", r.score, r.tab.title, r.tab.url)
            }).collect::<Vec<_>>().join("\n");
            let output = if text.is_empty() {
                format!("No results for '{}'", params.query)
            } else {
                format!("{} results for '{}':\n{}", results.len(), params.query, text)
            };
            Ok(CallToolResult::success(vec![Content::text(output)]))
        }

        #[tool(name = "list_groups", description = "List tab groups with optional label filter")]
        async fn list_groups(
            &self,
            Parameters(params): Parameters<ListGroupsParams>,
        ) -> Result<CallToolResult, McpError> {
            let limit = params.limit.unwrap_or(50);
            let groups = self.store.get_all_groups().await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;
            let groups: Vec<_> = groups.into_iter()
                .filter(|g| params.filter.as_deref()
                    .map(|f| g.label.as_deref().unwrap_or("").contains(f))
                    .unwrap_or(true))
                .take(limit)
                .collect();
            if groups.is_empty() {
                return Ok(CallToolResult::success(vec![Content::text("No groups found.")]));
            }
            let text = groups.iter().map(|g| {
                format!("[{}] {} ({} tabs) — {}",
                    g.id.get(..8).unwrap_or(&g.id),
                    g.label.as_deref().unwrap_or("(unlabeled)"),
                    g.tabs.len(),
                    g.created_at.format("%Y-%m-%d"))
            }).collect::<Vec<_>>().join("\n");
            Ok(CallToolResult::success(vec![Content::text(
                format!("{} groups:\n{}", groups.len(), text)
            )]))
        }

        #[tool(name = "get_stats", description = "Get store statistics: group count, tab count, top domains, date range")]
        async fn get_stats(&self) -> Result<CallToolResult, McpError> {
            let stats = self.store.get_stats().await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;
            let mut out = format!("Groups: {}\nTabs: {}", stats.total_groups, stats.total_tabs);
            if let Some(oldest) = stats.oldest_group {
                out.push_str(&format!("\nOldest: {}", oldest.format("%Y-%m-%d")));
            }
            if let Some(newest) = stats.newest_group {
                out.push_str(&format!("\nNewest: {}", newest.format("%Y-%m-%d")));
            }
            if !stats.top_domains.is_empty() {
                out.push_str("\nTop domains:");
                for (domain, count) in stats.top_domains.iter().take(10) {
                    out.push_str(&format!("\n  {} ({})", domain, count));
                }
            }
            Ok(CallToolResult::success(vec![Content::text(out)]))
        }

        #[tool(name = "recover_from_browser", description = "Recover tabs from a browser OneTab LevelDB store and import to tablitz")]
        async fn recover_from_browser(
            &self,
            Parameters(params): Parameters<RecoverFromBrowserParams>,
        ) -> Result<CallToolResult, McpError> {
            let browser_enum = match params.browser.to_lowercase().as_str() {
                "chrome" => tablitz_recover::Browser::Chrome,
                "edge"   => tablitz_recover::Browser::Edge,
                "brave"  => tablitz_recover::Browser::Brave,
                "comet"  => tablitz_recover::Browser::Comet,
                other    => return Err(McpError::invalid_params(
                    format!("Unknown browser '{}'. Use: chrome, edge, brave, comet", other), None
                )),
            };
            let opts = tablitz_recover::RecoverOptions {
                browser: browser_enum,
                profile: params.profile.unwrap_or_else(|| "Default".to_string()),
                dry_run: false,
                db_path: None,
            };
            let session = tablitz_recover::recover(opts)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;
            let stats = self.store.insert_session(&session).await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;
            Ok(CallToolResult::success(vec![Content::text(format!(
                "Recovered from {}: {} groups, {} tabs inserted ({} groups, {} tabs skipped)",
                params.browser,
                stats.groups_inserted, stats.tabs_inserted,
                stats.groups_skipped,  stats.tabs_skipped,
            ))]))
        }

        #[tool(name = "import_onetab_export", description = "Import tabs from a OneTab export file (.txt pipe or .md markdown format)")]
        async fn import_onetab_export(
            &self,
            Parameters(params): Parameters<ImportOnetabExportParams>,
        ) -> Result<CallToolResult, McpError> {
            let pb = std::path::PathBuf::from(&params.path);
            let session = tablitz_recover::parse_onetab_export(&pb)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;
            let stats = self.store.insert_session(&session).await
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;
            Ok(CallToolResult::success(vec![Content::text(format!(
                "Imported from {}: {} groups, {} tabs ({} groups, {} tabs skipped)",
                params.path,
                stats.groups_inserted, stats.tabs_inserted,
                stats.groups_skipped,  stats.tabs_skipped,
            ))]))
        }
    }

    // Tool parameter schemas
    #[derive(Deserialize, JsonSchema)]
    struct SearchTabsParams {
        query: String,
        limit: Option<usize>,
    }

    #[derive(Deserialize, JsonSchema)]
    struct ListGroupsParams {
        filter: Option<String>,
        limit: Option<usize>,
    }

    #[derive(Deserialize, JsonSchema)]
    struct RecoverFromBrowserParams {
        browser: String,
        profile: Option<String>,
    }

    #[derive(Deserialize, JsonSchema)]
    struct ImportOnetabExportParams {
        path: String,
    }

    #[tool_handler(router = self.tool_router)]
    impl ServerHandler for TablitzMcpServer {
        fn get_info(&self) -> rmcp::model::ServerInfo {
            use rmcp::model::{Implementation, ServerCapabilities};
            rmcp::model::ServerInfo {
                protocol_version: Default::default(),
                capabilities: ServerCapabilities {
                    tools: Some(Default::default()),
                    ..Default::default()
                },
                server_info: Implementation {
                    name: "tablitz".to_string(),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    ..Default::default()
                },
                instructions: None,
            }
        }
    }
}

