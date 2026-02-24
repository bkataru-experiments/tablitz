# tablitz — Architecture

## Overview

tablitz is a Rust CLI and MCP server for recovering, managing, searching, and backing up OneTab browser extension data. It targets users who have years of tabs saved in OneTab and need reliable recovery, deduplication, and search capabilities.

## Workspace Structure

```
tablitz/
└── crates/
    ├── tablitz-core/    # Data models (Tab, TabGroup, TabSession)
    ├── tablitz-recover/ # LevelDB extraction + export file parsing
    ├── tablitz-store/   # libSQL-backed canonical store
    ├── tablitz-search/  # Fuzzy, full-text, and semantic search + dedup
    ├── tablitz-sync/    # Git-backed snapshot and restore
    └── tablitz-cli/     # CLI commands + MCP server (optional feature)
```

## Crate Responsibilities

### `tablitz-core`
Defines the shared data types used throughout the workspace:
- `Tab` — URL (typed), title, favicon_url, added_at, id
- `TabGroup` — id, label, created_at, tabs, pinned/locked/starred flags
- `TabSession` — version, source, groups, created_at, imported_at
- `SessionSource` — which browser/profile or file the session came from (`Chrome { profile }`, `Edge { profile }`, `Brave { profile }`, `Comet { profile }`, `OneTabExport { path }`, `TablitzNative { path }`, `Unknown`)
- Helper: `ms_timestamp_to_datetime(ms: i64) -> DateTime<Utc>` (OneTab stores timestamps in Unix milliseconds)

### `tablitz-recover`
Handles reading OneTab data from raw sources:
- **LevelDB extraction** (`extract_from_leveldb`): opens a browser's OneTab extension LevelDB store using `rusty_leveldb`, iterates all key-value pairs, filters for entries containing `tabGroups`, parses the JSON schema, deduplicates groups via a `seen_group_ids` HashSet
- **OneTab pipe-format export** (`parse_onetab_export`): parses the `URL | Title` pipe-separated format that OneTab's export produces; assigns stable FNV-1a hash-based IDs per file
- **OneTab markdown export** (`parse_markdown_export`): parses the `---\n## N tabs\n> timestamp\n[title](url)` format produced by the JS exporter
- **Browser path resolution**: cross-platform path lookup for Chrome, Edge, Brave, and Perplexity Comet extension LevelDB directories

**OneTab LevelDB schema:**
```json
{
  "tabGroups": [
    {
      "id": "string",
      "createDate": 1760074389851,
      "title": "optional label",
      "pinned": false,
      "locked": false,
      "starred": false,
      "tabsMeta": [
        { "id": "string", "url": "https://...", "title": "page title" }
      ]
    }
  ]
}
```

> **Note:** The value stored in LevelDB is double-encoded: OneTab serializes the JSON object to a string, then stores that string as a JSON value. The raw bytes start with `"{\` (outer quote, brace, backslash). tablitz unwraps the outer string before parsing.

### `tablitz-store`
SQLite-backed (via `libsql`) canonical store persisted at `~/.local/share/tablitz/tablitz.db` (Linux) or platform equivalent:
- Schema: `tab_groups` table (id, label, created_at, pinned, locked, starred, source_type, source_profile, source_path, imported_at) + `tabs` table (id, group_id, url, title, favicon_url, added_at, position)
- `insert_session`: idempotent insert with `INSERT OR IGNORE` — re-importing the same data is safe
- `replace_tabs_for_group`: transactional delete + re-insert (used by dedup)
- `search_by_url` / `search_by_title`: SQL `LIKE '%query%'` full-text search
- `get_stats`: total groups, total tabs, oldest/newest timestamps, top 10 domains

### `tablitz-search`
In-process search and data quality tools:
- **`FuzzySearcher`**: uses `nucleo` for fuzzy matching across all tab titles+URLs; returns scored `SearchResult` list
- **`TitleNormalizer`**: strips common noise from tab titles (site names after ` - `, ` | `, ` — `; trims whitespace; applies unicode normalization)
- **`DedupEngine`**: three strategies — `ExactUrl`, `NormalizedUrl` (strips query params/fragments, normalizes trailing slashes), `UrlAndTitle`; returns a `DedupResult` with original/deduplicated counts
- **`FullTextIndex`** (feature: `full-text`): `tantivy`-backed inverted index for substring/phrase search
- **`SemanticIndex`** (feature: `ai`): `usearch` + `fastembed` for embedding-based similarity search
- **`AutoCategorizer`** (feature: `ai`): suggests group labels from tab title/URL patterns using TF-IDF-style scoring

### `tablitz-cli`
The user-facing binary (`tablitz`):

| Command | Description |
|---|---|
| `recover` | Extract from browser LevelDB → import to store or save to file |
| `import` | Import from OneTab export file or LevelDB path |
| `export` | Export store to JSON / Markdown / TOML |
| `search <query>` | Fuzzy or full-text search |
| `list` | List tab groups with filters |
| `dedup` | Deduplicate and persist deduplicated tabs |
| `init` | Create config/data directories |
| `stats` | Show store statistics |
| `serve` | Start MCP server (feature: `mcp`) |
| `snapshot` | Create git-backed snapshot of the store |
| `restore` | Restore store from a git-backed snapshot |
| `snapshots` | List recent snapshots in a repo |

**Optional features:**
- `mcp` — enables the `serve` command and `rmcp`-based MCP server
- `mcp-http` — adds HTTP transport via `axum`
- `full-text` — enables tantivy full-text index
- `ai` — enables semantic search and auto-categorization

## Data Flow

```
Browser LevelDB ──┐
OneTab .txt export ─┤─→ tablitz-recover ──→ TabSession ──→ tablitz-store (SQLite)
Tablitz JSON export ┘                                              │
                                                                   ├──→ tablitz-search (query)
                                                                   ├──→ tablitz-cli (serve/export)
                                                                   └──→ tablitz-sync (git backup)
```

## Design Decisions

- **`INSERT OR IGNORE`** — idempotent imports; group IDs are the natural dedup key
- **FNV-1a hashing for import IDs** — pipe/markdown import files don't have stable IDs, so IDs are derived from `fnv1a_hash(file_content)` + group index + tab position, giving stable, content-addressed IDs across repeated imports
- **libSQL over rusqlite** — async-native, drop-in SQLite compatibility, forward-compatible with Turso cloud sync
- **nucleo for fuzzy search** — same engine used by Helix editor; handles Unicode, very fast on large datasets
- **MCP via rmcp** — exposes tablitz capabilities as AI-assistant tools; stdio transport for Claude Desktop / Claude Code integration
