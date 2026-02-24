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
        let source = tablitz_core::SessionSource::Chrome { profile: p };
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

async fn cmd_search(query: String, _mode: SearchMode, limit: usize) -> Result<()> {
    let store = tablitz_store::Store::open_default().await?;
    let session = store.get_session().await?;

    let results = tablitz_search::FuzzySearcher::search(&query, &session);
    let results: Vec<_> = results.into_iter().take(limit).collect();

    if results.is_empty() {
        println!("No results for '{}'", query);
        return Ok(());
    }

    println!("{} results for '{}':", results.len(), query.bold());
    for r in &results {
        println!(
            "  [{:.2}] {} \n        {}",
            r.score,
            r.tab.title.cyan(),
            r.tab.url.as_str().dimmed()
        );
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
            group.id[..8].dimmed(),
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

    let stats = store.insert_session(&result.session).await?;
    println!("{} Saved deduplicated session ({} groups inserted)", "✓".green(), stats.groups_inserted);
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
