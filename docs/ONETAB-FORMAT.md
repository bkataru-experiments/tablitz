# OneTab Data Formats

Documentation of the data formats OneTab uses for storage and export/import, as reverse-engineered for tablitz.

## LevelDB Storage (Primary — Chrome Extension Storage)

OneTab stores its data in Chrome's extension LevelDB store at:

| Platform | Path |
|---|---|
| Windows (Chrome) | `%LOCALAPPDATA%\Google\Chrome\User Data\{Profile}\Local Extension Settings\chphlpgkkbolifaimnlloiipkdnihall\` |
| Windows (Edge) | `%LOCALAPPDATA%\Microsoft\Edge\User Data\{Profile}\Local Extension Settings\hoimpamkkoehapgenciaoajfkfkpgfop\` |
| Windows (Brave) | `%LOCALAPPDATA%\BraveSoftware\Brave-Browser\User Data\{Profile}\Local Extension Settings\chphlpgkkbolifaimnlloiipkdnihall\` |
| Linux (Chrome) | `~/.config/google-chrome/{Profile}/Local Extension Settings/chphlpgkkbolifaimnlloiipkdnihall/` |
| Linux (Edge) | `~/.config/microsoft-edge/{Profile}/Local Extension Settings/hoimpamkkoehapgenciaoajfkfkpgfop/` |
| Linux (Brave) | `~/.config/BraveSoftware/Brave-Browser/{Profile}/Local Extension Settings/chphlpgkkbolifaimnlloiipkdnihall/` |
| macOS (Chrome) | `~/Library/Application Support/Google/Chrome/{Profile}/Local Extension Settings/chphlpgkkbolifaimnlloiipkdnihall/` |

The OneTab extension ID is `chphlpgkkbolifaimnlloiipkdnihall` (consistent across browsers).

### LevelDB JSON Schema

The value stored under the main key in LevelDB is a JSON object:

```json
{
  "tabGroups": [
    {
      "id": "unique-string-id",
      "createDate": 1760074389851,
      "title": "optional group label",
      "pinned": false,
      "locked": false,
      "starred": false,
      "tabsMeta": [
        {
          "id": "tab-unique-id",
          "url": "https://example.com/page",
          "title": "Page Title"
        }
      ]
    }
  ]
}
```

> **Important:** The LevelDB value is **double-encoded** — OneTab stores the JSON as a JSON string (i.e., the raw bytes are an outer JSON string whose content is the serialized JSON object). When reading raw bytes the value starts with `"{\` rather than `{`. tablitz handles this transparently by first parsing the outer string, then parsing the inner JSON.

Key fields:
- `createDate` — Unix timestamp in **milliseconds** (not seconds)
- `title` — optional; may be `null` or absent
- `pinned`, `locked`, `starred` — boolean flags; may be absent (default false)
- `tabsMeta` — array of tabs; `id` fields are OneTab's internal IDs

### Recovery Notes

- The LevelDB directory should be **copied before reading** to avoid lock contention with a running browser
- tablitz uses `rusty_leveldb` (pure Rust, no C deps) with the copy placed in a temp directory
- Multiple keys may contain `tabGroups` data if OneTab has written multiple snapshots; tablitz deduplicates by group ID
- The `.log` file is the active write-ahead log; `.ldb` files are compacted SSTables. If the browser has not flushed/compacted recently (common with Edge), **all data may be in the `.log` file** with no `.ldb` files present — tablitz's `recover` command handles both cases

---

## Pipe-Separated Export Format (OneTab native export)

OneTab's built-in export produces a plain-text file where each tab group is separated by a blank line, and each tab is on its own line:

```
https://example.com/one | First Tab Title
https://example.com/two | Second Tab Title

https://example.com/three | Third Tab Title
https://example.com/four | Fourth Tab Title
```

Notes:
- Tab groups are separated by **blank lines**
- Each tab: `URL | Title` (pipe with spaces on both sides)
- No timestamps in this format (OneTab strips them on export — a known limitation)
- OneTab's import also accepts this format

---

## Markdown Export Format (JS exporter / tablitz export)

The enhanced export format produced by the DevTools JS exporter and by `tablitz export --format markdown`:

```markdown
---
## 8 tabs
> Created 3/20/2025, 10:08:46 PM

[Tab Title One](https://example.com/one)
[Tab Title Two](https://example.com/two)

---
## 4 tabs
> Created 3/19/2025, 6:43:59 PM
> Optional group label

[Another Tab](https://example.com/three)
```

Structure per group:
1. `---` separator
2. `## N tabs` header
3. `> Created M/D/YYYY, H:MM:SS AM/PM` timestamp (from OneTab's internal data)
4. `> Label` (optional, if group has a title)
5. Blank line
6. `[Title](URL)` for each tab
7. Trailing blank line

This format preserves timestamps, which the native pipe export does not.

---

## DevTools JS Exporter

A 26-line JavaScript snippet that extracts enhanced data directly from the OneTab extension page DOM:

**Usage:** Open the OneTab extension page → F12 → Console → type `allow pasting` → paste and run.

```javascript
let tabGroups = document.getElementsByClassName('tabGroup');

const headerTemplatizer = (tabCount, timestamp) => `
---
## ${tabCount}
> ${timestamp}

`;

const tabTemplatizer = (textContent, href) => `[${textContent}](${href})`;

function parseTabGroup(tabGroup) {
    let tabGroupString = '';
    const [headerElem, tabListElem] = tabGroup.children;
    const [tabCount, timestamp] = headerElem.innerText.split('\n').slice(0, 2);
    tabGroupString += headerTemplatizer(tabCount, timestamp);
    for (let tab of tabListElem.children) {
        const { textContent, href } = tab.getElementsByTagName('a')[0];
        tabGroupString += tabTemplatizer(textContent, href) + '\n';
    }
    return tabGroupString;
}

let output = '';
for (let tabGroup of tabGroups) {
    output += parseTabGroup(tabGroup);
}
copy(output); // copies markdown to clipboard
```

This is the recommended way to export if OneTab is still functional, as it preserves timestamps that the native export strips.

---

## tablitz Native Format

tablitz stores and exports its own JSON format (a serialized `TabSession`):

```json
{
  "version": 1,
  "source": { "Chrome": { "profile": "Default" } },
  "created_at": "2025-03-19T18:43:59Z",
  "imported_at": "2026-02-24T10:00:00Z",
  "groups": [
    {
      "id": "abc123",
      "label": "Optional label",
      "created_at": "2025-03-19T18:43:59Z",
      "pinned": false,
      "locked": false,
      "starred": false,
      "tabs": [
        {
          "id": "tab-abc-0",
          "url": "https://example.com",
          "title": "Example",
          "favicon_url": null,
          "added_at": "2025-03-19T18:43:59Z"
        }
      ]
    }
  ]
}
```
