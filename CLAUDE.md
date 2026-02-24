# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Check all crates compile
cargo check --workspace

# Check with MCP feature (required for serve command)
cargo check --features mcp -p tablitz-cli

# Run all tests
cargo test --workspace

# Run tests for a single crate
cargo test -p tablitz-store
cargo test -p tablitz-recover

# Run a single test by name
cargo test -p tablitz-store test_insert_and_retrieve

# Build release binary with MCP support
cargo build --release --features mcp

# Run the CLI in dev mode
cargo run -p tablitz-cli -- <command>
cargo run -p tablitz-cli --features mcp -- serve
```

## Architecture

6-crate workspace with a strict dependency DAG — no crate depends on a crate above it:

```
tablitz-core  (no workspace deps)
    ↓
tablitz-recover   tablitz-store   tablitz-search   tablitz-sync
    ↓                   ↓               ↓               ↓
                    tablitz-cli (depends on all four)
```

**tablitz-core** — Shared domain types only: `Tab`, `TabGroup`, `TabSession`, `SessionSource`. All `serde` timestamps use `ts_milliseconds` (OneTab's native format). IDs are `String` throughout. `TabSession::merge()` is the only cross-session operation here.

**tablitz-recover** — Two entry points:
- `recover(RecoverOptions)` — opens a live browser's LevelDB via `rusty_leveldb`, iterates all KV pairs, finds keys containing `tabGroups`. **The value is double-encoded**: OneTab stores it as a JSON string wrapping the JSON object (raw bytes start with `"{\`), so the code first calls `serde_json::from_str::<String>` to unwrap the outer string, then parses the inner JSON as `OneTabRoot`.
- `parse_onetab_export(path)` — dispatches to pipe-format (`URL | Title`) or markdown parser (`---\n## N tabs\n> timestamp`) based on file content; IDs are derived from FNV-1a hash of file content + position for stability

**tablitz-store** — Async `Store` wrapping a `libsql::Connection`. Default DB path: `~/.local/share/tablitz/tablitz.db`. Key behaviors:
- `insert_session` is fully idempotent (`INSERT OR IGNORE` on primary key); safe to call repeatedly
- `replace_tabs_for_group` does a transactional delete+reinsert (used after dedup)
- `search_by_url` / `search_by_title` are SQL `LIKE '%query%'` — not FTS5
- `get_session()` reconstructs a `TabSession` from the full store contents

**tablitz-search** — Pure in-memory operations over a `TabSession`:
- `FuzzySearcher::search` uses `nucleo` (same engine as Helix); operates on title+URL concatenated
- `TitleNormalizer::normalize` strips known site-name suffixes (` - SiteName`, ` | SiteName`, etc.) and trims whitespace; only strips suffixes it recognizes — does not strip arbitrary patterns
- `DedupEngine::dedup` with `NormalizedUrl` strips `utm_*` query params but not arbitrary params like `?ref=`
- Optional features: `full-text` (tantivy), `ai` (fastembed + usearch)

**tablitz-sync** — `SyncManager` wraps a git repo path and calls `git` as a subprocess. `snapshot()` serializes the full store session to JSON and commits; `restore()` reads the JSON back and calls `insert_session`.

**tablitz-cli** — Single binary `tablitz` at `crates/tablitz-cli/src/main.rs`. The MCP server (`mod mcp`) is behind `#[cfg(feature = "mcp")]`. MCP uses `rmcp 0.10` with the `Parameters<T>` struct pattern (not individual `#[tool(description)]` on plain params) — parameter structs derive `Deserialize + JsonSchema` using `use rmcp::schemars::JsonSchema`.

## Feature Flags

Defined in `tablitz-cli/Cargo.toml`:
- `mcp` — enables `serve` command and `mod mcp` (rmcp server)
- `mcp-http` — adds axum HTTP transport on top of `mcp`
- `full-text` — passes through to `tablitz-search/full-text` (tantivy)
- `ai` — passes through to `tablitz-search/ai` (fastembed + usearch)

## Key Constraints

- **MCP rmcp 0.10**: `#[tool_router]` on the impl block generates `Self::tool_router()`. `#[tool_handler(router = self.tool_router)]` on `impl ServerHandler` auto-generates `call_tool` and `list_tools` — do not implement those manually. `schemars` is re-exported from rmcp as `rmcp::schemars`, not a direct dep.
- **Idempotent imports**: Group IDs are the dedup key. The same data can be imported multiple times safely.
- **Timestamps**: All datetimes stored as Unix milliseconds in SQLite (`INTEGER`); `chrono::serde::ts_milliseconds` handles serialization.
- **Async runtime**: `tablitz-store` and `tablitz-sync` are async; tests use `#[tokio::test]` with `rt-multi-thread` and `macros` features in dev-dependencies.
