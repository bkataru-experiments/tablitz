# tablitz

 Recover, manage, search, and back up your OneTab tabs — in Rust

[![MIT License](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

## What is tablitz?

tablitz is a Rust-powered CLI tool and MCP server for recovering, managing, and searching your OneTab browser tab data. It extracts 20,000+ tabs from Chrome/Edge/Brave LevelDB stores, stores them in a local SQLite database for fast querying, and exposes them to AI assistants like Claude Desktop and Claude Code through the Model Context Protocol.

## Features

- **LevelDB Recovery** — Extract OneTab data from Chrome, Edge, Brave, and Comet browser profiles
- **Import/Export** — OneTab pipe (.txt) and markdown (.md) format support, plus JSON/TOML
- **Powerful Search** — Fuzzy search with scoring and SQL full-text search across titles and URLs
- **Deduplication** — Three strategies: exact URL, normalized URL, URL+title combination
- **Multiple Export Formats** — Export tabs to JSON, Markdown, or TOML
- **Git-Backed Snapshots** — Version-controlled backups with full restore capability
- **MCP Server Integration** — Expose your tab collection to Claude Desktop and Claude Code

## Installation

Build from source with the MCP feature enabled:

```bash
cargo build --release --features mcp
```

The binary will be available at `target/release/tablitz`.

## Quick Start

```bash
# 1. Recover tabs from your browser
tablitz recover --browser chrome

# 2. View store statistics
tablitz stats

# 3. Search for tabs
tablitz search "rust tutorial"

# 4. Export results
tablitz export --format markdown --out tabs.md
```

## Commands Reference

| Command | Description | Key Flags |
|---------|-------------|-----------|
| `recover` | Recover OneTab data from browser LevelDB | `--browser`, `--profile`, `--dry-run`, `--out` |
| `import` | Import tab data into the store | `--from-onetab-export`, `--from-onetab-leveldb` |
| `export` | Export tab data from the store | `--format`, `--out`, `--filter` |
| `search` | Search tabs with fuzzy or full-text mode | `--mode`, `--limit` |
| `list` | List tab groups with optional filtering | `--filter`, `--limit` |
| `dedup` | Deduplicate tabs using configurable strategy | `--strategy`, `--normalize-titles`, `--dry-run` |
| `init` | Initialize tablitz config and data directories | (none) |
| `stats` | Show store statistics and top domains | (none) |
| `serve` | Start MCP server for AI assistant integration | `--port` |
| `snapshot` | Create git-backed snapshot of the store | `--repo`, `--filename` |
| `restore` | Restore store from git-backed snapshot | `--repo`, `--commit`, `--filename` |
| `snapshots` | List recent snapshots in a git repo | `--repo`, `--limit` |

## MCP Server

To use tablitz with Claude Desktop or Claude Code, add it to your MCP configuration:

```json
{
  "mcpServers": {
    "tablitz": {
      "command": "tablitz",
      "args": ["serve"]
    }
  }
}
```

The MCP server exposes these tools:
- `search_tabs` — Fuzzy search tabs by query
- `list_groups` — List tab groups with optional filtering
- `get_stats` — Get store statistics and top domains
- `recover_from_browser` — Recover tabs from browser LevelDB
- `import_onetab_export` — Import from OneTab export files

## Data Formats

tablitz understands OneTab's native formats. See [docs/ONETAB-FORMAT.md](docs/ONETAB-FORMAT.md) for detailed format specifications.

## Architecture

tablitz uses a multi-crate workspace architecture:

- `tablitz-core` — Shared types, session models, and domain logic
- `tablitz-recover` — LevelDB extraction and OneTab format parsing
- `tablitz-store` — SQLite persistence with FTS5 full-text search
- `tablitz-search` — Fuzzy search, deduplication, and title normalization
- `tablitz-sync` — Git-backed snapshot and restore functionality
- `tablitz-cli` — Command-line interface and MCP server

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for architectural details.

## Recovery Guide

Need to recover OneTab data from your browser? See [docs/RECOVERY.md](docs/RECOVERY.md) for step-by-step instructions covering browser profiles, LevelDB paths, and troubleshooting.

## License

MIT License — see [LICENSE](LICENSE) for details.
