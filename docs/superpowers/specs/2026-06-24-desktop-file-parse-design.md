# `madft app <app> desktop` — faithful `.desktop` file parse

**Date:** 2026-06-24
**Status:** Approved, ready for implementation plan

## Summary

Add a `desktop` action to the `app` subcommand that parses an application's
`.desktop` file and prints its sections, keys, and values. The parse is
**faithful**: values are raw strings (no type coercion, no `Exec` splitting),
keys are kept verbatim, and section/key order is preserved from the file.
Optional positional field names select specific keys for scripting convenience.

```
madft app nvim desktop                 # full dump
madft app nvim desktop --json          # full dump as JSON
madft app nvim desktop Exec            # one field, raw value
madft app nvim desktop Terminal Exec   # several fields, one per line
```

## Design rationale: faithful, not interpreted

The command is an **inspector**, and an inspector's value is showing the file
truthfully. We deliberately do **not** interpret values, because the
`.desktop` format is open even though its known schema is stable:

- **`X-` extension keys** (`X-GNOME-Autostart`, …) have no defined type.
- **Localized keys** (`Name[de]=…`) make the key something other than `Name`.
- **Action sections** (`[Desktop Action new-window]`) carry their own keys.
- **Standard keys grow over time** across spec revisions.

A typed model would have to drop or mangle everything it doesn't model,
breaking the one promise this command makes — "list all fields and their
values". Raw strings are also already madft's scriptable primitive (`get`
returns the raw default string). `Exec` in particular is a real grammar
(quoting + field codes like `%F`/`%U`); naive splitting is wrong for many real
files and correct splitting is pointless work for an inspector — the raw string
is the truth.

## Components

### 1. `src/desktop.rs` — the parser (new module)

```rust
pub struct DesktopFile {
    pub path: String,
    pub sections: Vec<DesktopSection>,    // file order
}
pub struct DesktopSection {
    pub name: String,                     // e.g. "Desktop Entry", "Desktop Action new-window"
    pub entries: Vec<(String, String)>,   // file order; key and value both trimmed
}

pub fn parse(content: &str) -> DesktopFile  // path set by caller
```

Parse rules:

- `[Header]` (line that starts with `[` and ends with `]`) opens a new section;
  the header text between the brackets is the section name.
- `key=value` splits on the **first** `=`; key and value are trimmed.
- Blank lines and lines starting with `#` are skipped.
- Lines before the first header are ignored.
- Keys are kept **verbatim** — `X-Foo`, `Name[de]`, mixed case all preserved.
- **First occurrence of a key wins** within a section (dedup-on-parse). This
  keeps the emitted JSON object valid (no duplicate keys) without reordering.
  Keys differing only by case (`Exec` vs `exec`) are distinct and both kept.

### 2. Serialization — order-preserving, no new dependency

The dependency list is intentionally lean (clap, serde, serde_json, thiserror,
toml). Rather than add `indexmap`, hand-write a `Serialize` impl for
`DesktopFile` that emits `sections` and each section's `entries` as JSON
**objects** via `serialize_map`, driven directly from the ordered `Vec`s. Order
lives in the data structure, so output is insertion-ordered without a serializer
feature.

```json
{
  "path": "/usr/share/applications/nvim.desktop",
  "sections": {
    "Desktop Entry": { "Name": "Neovim", "Exec": "nvim %F", "Terminal": "true" }
  }
}
```

### 3. `src/appindex.rs` — store path, reuse the parser

- Add `pub path: PathBuf` to `App`. The path is already in scope in `load()`;
  it is currently dropped.
- Refactor `parse_desktop` to call `desktop::parse`, then derive
  `Name` / `NoDisplay` / `MimeType` from the `Desktop Entry` section (preserving
  today's semantics: first `Name` wins, `NoDisplay` is `eq_ignore_ascii_case`
  to "true", `MimeType` is `;`-split into a `HashSet`). Removes the duplicate
  hand-rolled INI loop. Existing appindex tests guard the behavior.

### 4. `src/engine.rs` — new method

```rust
pub fn desktop(&self, id: &str) -> Result<DesktopFile>
```

Resolves the `DesktopId` like `app()` does (`Error::UnknownApp` if absent),
reads the file at the stored `App.path`, and returns `desktop::parse` with
`path` populated. I/O failures flow through the existing `Error::Io`.

### 5. `src/cli.rs` — nested subcommand under `App`

```rust
App {
    id: String,
    #[command(subcommand)]
    action: Option<AppAction>,   // None → today's AppReport, unchanged
}

#[derive(Subcommand, Debug)]
pub enum AppAction {
    /// Show the parsed .desktop file, or select specific fields
    Desktop { fields: Vec<String> },
}
```

- `madft app nvim` → unchanged `AppReport`.
- `madft app nvim desktop` → full dump.
- `madft app nvim desktop Exec Terminal` → selected fields.

**Field selection is case-sensitive**, matching exact keys in the
`[Desktop Entry]` section only. This is consistent with app IDs, which are
already case-sensitive (`net.thunderbird.Thunderbird`). `exec` will not match
`Exec`.

### 6. Output rendering

| Command | Human | `--json` |
|---|---|---|
| `desktop` (full) | INI reproduction: `[Section]` header then `Key=Value` lines, sections/keys in file order | `{ "path", "sections": { … } }` |
| `desktop <fields>` | each selected raw value, one per line, in the order requested | `{ "<Key>": "<value>", … }` keyed by the matched (verbatim) key |

Edge behavior, following existing `get` precedent (raw value / empty string in
human mode, `null` in JSON):

- **Missing field**, human → empty line; `--json` → `null`.
- **`Terminal` prints `true`** verbatim — no bool coercion, no `True`.

## Testing

- **Unit tests** in `desktop.rs`: header detection, comment/blank skipping,
  first-key-wins dedup, `X-` keys, localized keys (`Name[de]`), an
  `[Desktop Action …]` section, value/section ordering, lines before first
  header ignored, `split_once('=')` with `=` in the value.
- **Golden integration tests** in `tests/golden.rs`: the four output paths
  (full human, full JSON, field-select human, field-select JSON), plus
  unknown-app error and a missing-field case. Add a fixture `.desktop` that
  includes an `X-` key and a `[Desktop Action …]` section to prove faithfulness.
- Existing `appindex.rs` tests must pass unchanged after the parser refactor.

## Out of scope (YAGNI)

- Type coercion of any value (booleans, numbers, lists).
- `Exec` tokenization / field-code expansion.
- Locale resolution / `--locale` filtering (all localized keys shown verbatim).
- Writing or editing `.desktop` files — read-only inspection.
