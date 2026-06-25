# madft

**m**ime **a**pp **d**efault **f**or **t**erminal — inspect and set your XDG default applications, organized around a human-curated **category tree** (`Media.Video → video/mp4`) instead of a flat wall of mimetypes.

`gio` and `xdg-mime` already answer *"what could open this file?"*. `madft` answers the other question — *"what should be the **default**, and how do I curate that across everything?"* — with a navigable category tree, exact-declaration semantics, correct XDG precedence, and atomic, backed-up writes. It's designed to be a stable machine-facing API (`--json`) that a TUI or other front-end can sit on top of.

> Status: v0.5.0. 9 commands, human + `--json` output, ~144 tests. Single self-contained binary; the only thing it mutates is `~/.config/mimeapps.list`.

## Install

Requires **Rust 1.85+** (edition 2024).

```bash
git clone <your-repo-url> madft && cd madft
cargo install --path .          # installs `madft` to ~/.cargo/bin
# or just build:  cargo build --release   ->  target/release/madft
```

## Quick start

`madft` works out of the box with a **built-in default category tree** covering ~570 curated types across Media, Images, Documents, Text, Web, Archives, Fonts, Models, Data, Software, Personal, Games, Mail, Security, and Media.Subtitles — so `ls` shows sensible groups immediately, no setup needed. The curated tree is also the source of truth for types that `file`/`xdg-open` emit via content-sniffing (libmagic) but that shared-mime-info doesn't register — for example `font/sfnt`, which `.ttf` files resolve to when sniffed, is listed explicitly so font-opener defaults apply correctly. Anything not in the tree still has a home — it falls into a flat `Other` node — and `-a`/`--all` reveals everything, including types with no installed app (hidden by default; see Presence filter below).

To customize, drop an editable copy of the default on disk and edit it (`overrides.toml` is also merged on top, if present):

```bash
madft init                                  # writes ~/.local/share/madft/categories.toml
$EDITOR ~/.local/share/madft/categories.toml
```

Everyday use:

```bash
madft ls                       # top-level categories (installed apps only)
madft ls --all                 # include types with no installed handler (full taxonomy)
madft ls Media                 # Media.Audio  Media.Video
madft ls Media.Video           # video/mp4  [default: mpv.desktop, apps: 3]  ...
madft info video/mp4           # category, default, applicable apps, inherit-if-unset chain
madft apps                     # all apps, ranked by coverage (whole tree)
madft apps Images              # apps that handle the umbrella, ranked by coverage
madft app mpv                  # the inverse: what mpv declares + what it's default for
madft get video/mp4            # bare default (scriptable)

madft set mpv                  # make mpv default for everything it declares (system-wide)
madft set mpv Media            # scope to a category (only types mpv declares)
madft set mpv video/mp4        # set one type
madft set swayimg --no-clobber # fill only image types that have no default yet
madft unset video/mp4          # remove a user default

# Pipe a computed list of types (newline-delimited) into set/unset:
madft app firefox --json | jq -r '.types[] | select(.category==null) | .mime' \
  | madft set firefox - --no-clobber   # set firefox for its uncategorized types
madft types Media.Video | madft unset -   # clear defaults for every video type
```

Add `--json` to any command for machine-readable output.

## Commands

| Command | What it does |
|---|---|
| `ls [PATH]` | Child categories + leaf types at a node (roots if no PATH); each leaf shows its current default + applicable-app count. Hides app-less types/categories unless --all. |
| `types <PATH>` | All mimetypes under the umbrella (recursive, alias-canonicalized). |
| `info <mimetype>` | Canonical name, **category**, **effective default** (`{app, via}` — `via` set when inherited from a parent type), apps that declare it, apps that could open it via inheritance, and the `inherits if unset` chain. |
| `apps [PATH\|mimetype]` | Apps that declare any of the umbrella's types, ranked by coverage. With no target, the whole tree (`.` is an explicit root alias). |
| `app <id>` | One app's mimetypes: those it declares **and** those it's the current default for (even undeclared ones, flagged `declares: false` / `(not declared)` and marked `DEFAULT`), plus the category each falls in. |
| `app <id> desktop [fields…]` | Parse the app's `.desktop` file. With no fields, dumps all sections and keys in file order (use `--json` for machine output). With field names (case-sensitive, from `[Desktop Entry]`), prints just those raw values one per line — handy for scripts without `jq`. |
| `get <mimetype>` | The bare current default id (empty if unset). Scriptable. |
| `set <app> [PATH\|mimetype\|-] [--types a,b] [-f/--force] [--no-clobber] [--exact] [--dry-run]` | Set `app` as default for the umbrella's types it handles — declared **or** reachable via a parent type. A `-` target (or piped stdin with no target) reads a **newline-delimited mimetype list from stdin**, which becomes the operand set directly (bypasses the category tree — good for uncategorized types); incompatible with `--types`. `--exact` restricts to literally-declared types; `--force` overrides entirely; `--no-clobber` fills only unset; `--dry-run` previews. |
| `unset [mimetype\|-]` | Remove the user default for a type. A `-` argument (or piped stdin with no argument) removes the default for **each mimetype on stdin** (newline-delimited), reporting per type. |
| `init [-f/--force]` | Write the built-in default category tree to `~/.local/share/madft/categories.toml` for editing (no-op if it exists, unless `--force`). |

Exit codes: `0` success, `1` on an operational error (unknown path/app, guard), `2` on a usage error.

Global flags: `--json` (machine output) and `-a`/`--all` (include types/categories with no installed app; off by default) work on every listing command and `set`.

## How it works

- **Two distinct trees.** The **category tree** is your human navigation overlay (`Media.Video`); the **freedesktop subclass DAG** (`text/html → text/plain`, plus aliases like `image/jpg → image/jpeg`) is a separate axis. They stay distinct, but the DAG is no longer just an annotation — it drives openability, the effective default, and `set` (see *Subclass inheritance* below).
- **Exact-declaration, plus inheritance.** "App X *declares* type T" means X's `.desktop` file *explicitly* lists T in `MimeType=`. By default `set` covers types the app declares **or can open via a parent type**; `--exact` restricts to literal declarations, and `--force` overrides the guard entirely.
- **Total tree.** Every type in your system's MIME database resolves to some node — unplaced types fall to a flat `Other`, so nothing is invisible. The built-in tree (~570 types in v0.4.0) also first-classes types that `file`/`xdg-open` emit via libmagic content-sniffing but that shared-mime-info doesn't register (e.g. `font/sfnt` — what a `.ttf` becomes when sniffed). Listing them in the curated tree means `madft set` covers them and content-sniffing openers resolve to the right app.
- **Presence filter.** By default `madft` shows only what your machine can act on — types with at least one installed app, and categories that contain such a type. The built-in tree is deliberately comprehensive (the long tail still lands in `Other`), so pass **`-a`/`--all`** to any listing or `set` to see/act on the full taxonomy. Naming a mimetype explicitly (or via `--types`) always works, filtered or not.
- **Subclass inheritance is real.** A type you can open *via an ancestor* (your editor handles `text/plain`, so it opens `text/x-python`) counts as openable — it shows in `ls`, gets an **effective default** in `info` (`default: nvim.desktop (via text/plain)`), and `set` will set your app for it without `--force`. Pass **`--exact`** to `set` to restrict to types the app declares literally (leaving inherited-only types blank, still inheriting).
- **Correct XDG precedence.** `~/.local/share` shadows system dirs (the *correct* direction — not the inverted first-seen behavior some launchers use).

### Configuration & files

| File | Role |
|---|---|
| `${XDG_DATA_HOME:-~/.local/share}/madft/categories.toml` | The category tree (shared/maintained layer). If absent, a built-in default is used; `madft init` writes an editable copy here. |
| `${XDG_CONFIG_HOME:-~/.config}/madft/overrides.toml` | Personal re-placements (same grammar; wins over the defaults layer). |
| `${XDG_CONFIG_HOME:-~/.config}/mimeapps.list` | The **only** file `madft` writes (its `[Default Applications]` section). |

A type listed under two paths in one file is a load error; an override file simply moves a type to a new node. Category names allow `[A-Za-z0-9 _-]` (no `.`, `:`, `/`).

### Write safety

`madft` edits only the `[Default Applications]` section of `~/.config/mimeapps.list` — every other section, key, comment, and ordering round-trips verbatim. Writes are **atomic** (temp file + `fsync` + rename), **backed up** (`mimeapps.list.bak` before writing), and **idempotent** (setting an existing value writes nothing). It never touches system files or `[Added]`/`[Removed]` associations.

## The `--json` contract

Every command emits a stable, additive JSON schema for scripting and front-ends:

```jsonc
// madft set mpv Media --no-clobber --dry-run --json
{
  "app": "mpv.desktop",
  "target": "Media",
  "set_types": ["audio/mpeg", "video/x-matroska"],
  "skipped_types": ["image/png", "image/jpeg"],
  "unchanged_types": ["video/mp4"],
  "inherited_via": [],
  "forced": false,
  "no_clobber": true,
  "dry_run": true,
  "written": false
}
```

```jsonc
// madft app nvim --json   (after: madft set nvim text/css --force)
{
  "id": "nvim.desktop",
  "name": "Neovim",
  "declares": 1,
  "default_for": 1,
  "types": [
    {"mime": "text/css",   "category": "Web",       "declares": false, "is_default": true,  "current_default": "nvim.desktop"},
    {"mime": "text/plain", "category": "Documents", "declares": true,  "is_default": false, "current_default": null}
  ]
}
```

```jsonc
// madft app nvim desktop --json
{
  "path": "/usr/share/applications/nvim.desktop",
  "sections": {
    "Desktop Entry": {"Name": "Neovim", "Exec": "nvim %F", "Terminal": "true", "MimeType": "text/plain;"},
    "Desktop Action new-window": {"Name": "Open a New Window", "Exec": "nvim"}
  }
}
```

```jsonc
// madft info application/xml --json   (with text/plain set to nvim)
{
  "mime": "application/xml",
  "category": "Other",
  "comment": null,
  "default": {"app": "nvim.desktop", "via": "text/plain"},
  "applicable_count": 0,
  "inheritable_count": 1,
  "ancestor_types": ["text/plain"],
  "applicable_apps": [],
  "inheritable_apps": [{"id": "nvim.desktop", "name": "Neovim", "via": "text/plain"}]
}
```

Errors emit `{"error": {"kind": "...", "message": "..."}}` on stdout with a non-zero exit.

## Architecture

A single binary over an internal library, layered so the engine is testable independently of the CLI and reusable by a future front-end:

- `mimedb` — freedesktop MIME facts (type universe, aliases, subclass DAG).
- `appindex` — exact-declaration index over `applications/*.desktop`.
- `defaults` — the effective current default from the `mimeapps.list` chain.
- `categories` — the layered category tree (arena model + TOML `Source` + `defaults ← overrides ← Other` merge).
- `writer` — pure `apply(content, edits)` transform + atomic, backed-up I/O.
- `engine` — orchestrates the above into the operations; returns serializable result structs.
- `cli` — clap subcommands; human vs `--json` rendering.

Every reader takes an injectable XDG root set, so the test suite runs entirely against committed fixtures with zero reliance on the host system.

## Development

```bash
cargo test                                  # unit + golden integration tests
cargo clippy --all-targets -- -D warnings   # lints clean
```

## Not (yet) included

Deferred by design: a remote/community category database (the `Source` trait seam exists), a TUI front-end, fuzzy app-name matching, `[Added]`/`[Removed]` management, multi-app fallback lists, and file locking. Bulk "set everything unset" now ships as `set --no-clobber`; a dedicated "list unset" query stays composable from the `--json`/`ls` reads.

## License

[MIT](LICENSE) © samtell
