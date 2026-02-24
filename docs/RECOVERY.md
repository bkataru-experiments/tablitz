# OneTab Data Recovery Guide

This guide explains how to recover lost OneTab data using tablitz. This is particularly relevant after a OneTab update wipes the extension's active state — the underlying LevelDB data is almost always still on disk.

## Background

OneTab stores all tab group data in Chrome's extension LevelDB store on the local filesystem. When OneTab updates and appears to lose your data, the issue is typically that the new version started with a fresh state in the write-ahead log while your data remains intact in the compacted `.ldb` SSTable files.

The native `strings`/`grep` approach on `.ldb` files only recovers a fraction of URLs (~335 out of 20,000+) because LevelDB's SSTable format is binary. tablitz uses `rusty_leveldb` to decode the format properly.

## Quick Start

### Step 1: Find your LevelDB directory

**Chrome (Windows):**
```
%LOCALAPPDATA%\Google\Chrome\User Data\Default\Local Extension Settings\chphlpgkkbolifaimnlloiipkdnihall\
```

**Chrome (Linux):**
```
~/.config/google-chrome/Default/Local Extension Settings/chphlpgkkbolifaimnlloiipkdnihall/
```

**Edge, Brave, Comet:** Replace the browser-specific path prefix (see [ONETAB-FORMAT.md](ONETAB-FORMAT.md)).

### Step 2: Initialize tablitz

```sh
tablitz init
```

### Step 3: Recover from your browser

```sh
# Recover from Chrome (default profile)
tablitz recover --browser chrome

# Recover from a specific profile
tablitz recover --browser chrome --profile "Profile 1"

# Recover from a specific LevelDB path (if auto-detection fails)
tablitz recover --db-path /path/to/leveldb/directory

# Dry run — see what would be recovered without importing
tablitz recover --browser chrome --dry-run

# Save to a JSON file instead of importing to store
tablitz recover --browser chrome --out my_tabs.json
```

### Step 4: Verify the recovery

```sh
tablitz stats
tablitz list --limit 20
```

### Step 5: Export your data

```sh
# Export to Markdown (human-readable)
tablitz export --format markdown --out my_tabs.md

# Export to JSON (machine-readable, re-importable)
tablitz export --format json --out my_tabs.json

# Export to TOML
tablitz export --format toml --out my_tabs.toml
```

---

## Recovering from Multiple Browsers

If you had OneTab installed on multiple browsers, recover from each:

```sh
tablitz recover --browser chrome
tablitz recover --browser edge
tablitz recover --browser brave
tablitz recover --browser comet
```

Duplicate groups are automatically skipped (idempotent import by group ID).

---

## Recovering from an Existing Export File

If you have a `.txt` file from OneTab's built-in export:

```sh
tablitz import --from-onetab-export my_onetab_backup.txt
```

If you have a Markdown export from the JS exporter:

```sh
# Markdown exports are detected automatically by file content
tablitz import --from-onetab-export my_tabs.md
```

If you have a LevelDB directory you copied manually:

```sh
tablitz import --from-onetab-leveldb /path/to/copied/leveldb --browser chrome --profile Default
```

---

## Searching Your Recovered Tabs

```sh
# Fuzzy search (default)
tablitz search "rust async"

# Full-text search
tablitz search "leveldb" --mode full-text

# Limit results
tablitz search "typescript" --limit 50
```

---

## Deduplication

After recovering from multiple sources, deduplicate:

```sh
# Preview what would be removed
tablitz dedup --dry-run

# Deduplicate by normalized URL (strips query params, trailing slashes)
tablitz dedup --strategy normalized-url

# Also normalize titles (strips site names like " - GitHub", " | DEV")
tablitz dedup --strategy normalized-url --normalize-titles

# Strict: only deduplicate identical URLs
tablitz dedup --strategy exact-url
```

---

## Troubleshooting

### "Permission denied" / "Database locked"

The browser must be fully closed before tablitz can read its LevelDB store. tablitz copies the directory to a temp location before reading, but the copy itself may fail if Chrome holds locks on the files.

**Fix:** Close the browser completely (check Task Manager / `ps aux`), then run again.

### Recovery finds 0 groups

The extension LevelDB directory may be in a different profile. Check:

```sh
# List available Chrome profiles
ls "~/.config/google-chrome/"

# Try each profile
tablitz recover --browser chrome --profile "Profile 1"
tablitz recover --browser chrome --profile "Profile 2"
```

Or specify the exact path:
```sh
tablitz recover --db-path ~/.config/google-chrome/Profile\ 2/Local\ Extension\ Settings/chphlpgkkbolifaimnlloiipkdnihall/
```

### Edge: no `.ldb` files found

Edge may not have compacted its LevelDB yet, so all data is in the write-ahead log (`.log` file) rather than `.ldb` SSTables. tablitz's `recover --browser edge` reads both; if it finds 0 groups, ensure the Edge extension directory is the correct one:

```
%LOCALAPPDATA%\Microsoft\Edge\User Data\{Profile}\Local Extension Settings\hoimpamkkoehapgenciaoajfkfkpgfop\
```

Note the extension ID `hoimpamkkoehapgenciaoajfkfkpgfop` is specific to Edge (Chrome uses `chphlpgkkbolifaimnlloiipkdnihall`).

### Auto-detected path is wrong

Use `--db-path` to specify the exact LevelDB directory manually.

---

## MCP Server (AI Assistant Integration)

tablitz can run as an MCP server, exposing search and recovery tools to Claude Desktop, Claude Code, or other MCP clients:

```sh
# Build with MCP support
cargo build --features mcp

# Run as MCP server (stdio transport — for Claude Desktop config)
tablitz serve
```

Add to Claude Desktop's `claude_desktop_config.json`:
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

Available MCP tools: `search_tabs`, `list_groups`, `get_stats`, `recover_from_browser`, `import_onetab_export`.
