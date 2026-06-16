# madft

**m**ime **a**pp **d**efault **f**or **t**erminal — inspect and set your XDG default applications, organized around a human-curated **category tree** (`Media.Video → video/mp4`) instead of a flat wall of mimetypes.

`gio` and `xdg-mime` already answer *"what could open this file?"*. `madft` answers the other question — *"what should be the **default**, and how do I curate that across everything?"* — with a navigable category tree, exact-declaration semantics, correct XDG precedence, and atomic, backed-up writes. It's designed to be a stable machine-facing API (`--json`) that a TUI or other front-end can sit on top of.

> Status: MVP complete. 8 commands, human + `--json` output, ~83 tests. Single self-contained binary; the only thing it mutates is `~/.config/mimeapps.list`.

## Install

Requires **Rust 1.85+** (edition 2024).

```bash
git clone <your-repo-url> madft && cd madft
cargo install --path .          # installs `madft` to ~/.cargo/bin
# or just build:  cargo build --release   ->  target/release/madft
```

## Quick start

`madft` works out of the box with a **built-in default category tree**, so `ls` shows sensible groups (Media, Images, Documents, Web, Archives…) immediately — no setup. Anything not placed in the tree falls into a flat `Other` node, so nothing is ever hidden.

To customize, drop an editable copy of the default on disk and edit it (`overrides.toml` is also merged on top, if present):

```bash
madft init                                  # writes ~/.local/share/madft/categories.toml
$EDITOR ~/.local/share/madft/categories.toml
```

Everyday use:

```bash
madft ls                       # top-level categories
madft ls Media                 # Media.Audio/  Media.Video/
madft ls Media.Video           # video/mp4  [default: mpv.desktop, apps: 3]  ...
madft info video/mp4           # category, default, applicable apps, inherit-if-unset chain
madft apps Images              # apps that handle the umbrella, ranked by coverage
madft app mpv                  # the inverse: what mpv declares + what it's default for
madft get video/mp4            # bare default (scriptable)

madft set video/mp4 mpv        # set one type
madft set Media mpv            # set the whole umbrella (only types mpv declares)
madft set image/png swayimg    # writes ~/.config/mimeapps.list atomically, keeps a .bak
madft unset video/mp4          # remove a user default
```

Add `--json` to any command for machine-readable output.

## Commands

| Command | What it does |
|---|---|
| `ls [PATH]` | Child categories + leaf types at a node (roots if no PATH); each leaf shows its current default + applicable-app count. |
| `types <PATH>` | All mimetypes under the umbrella (recursive, alias-canonicalized). |
| `info <mimetype>` | Canonical name, **category**, current default, applicable apps, and the `ancestor_types` (inherit-if-unset) chain. |
| `apps <PATH\|mimetype>` | Apps that declare any of the umbrella's types, ranked by coverage. |
| `app <id>` | One app's declared types, the category each falls in, and which it's currently the default for. |
| `get <mimetype>` | The bare current default id (empty if unset). Scriptable. |
| `set <PATH\|mimetype> <app> [--types a,b] [-f/--force] [--dry-run]` | Set `app` as default for the umbrella's declared types. Reports skipped (undeclared) types — not an error. `--force` overrides the declaration guard; `--dry-run` previews. |
| `unset <mimetype>` | Remove the user default for a type. |
| `init [-f/--force]` | Write the built-in default category tree to `~/.local/share/madft/categories.toml` for editing (no-op if it exists, unless `--force`). |

Exit codes: `0` success, `1` on an operational error (unknown path/app, guard), `2` on a usage error.

## How it works

- **Two trees, never conflated.** The **category tree** is your human navigation overlay (`Media.Video`). The **freedesktop subclass DAG** (`text/html → text/plain`, plus aliases like `image/jpg → image/jpeg`) is surfaced read-only as the "what you'd inherit if unset" annotation.
- **Exact-declaration.** "App X handles type T" means X's `.desktop` file *explicitly* lists T in `MimeType=`. `set` only sets types the app declares (unless you `--force`); inheritance is never a set target.
- **Total tree.** Every type in your system's MIME database resolves to some node — unplaced types fall to a flat `Other`, so nothing is invisible.
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
// madft app swayimg --json
{
  "id": "swayimg.desktop",
  "name": "swayimg",
  "declares": 5,
  "default_for": 2,
  "types": [
    {"mime": "image/png",  "category": "Images", "is_default": true,  "current_default": "swayimg.desktop"},
    {"mime": "image/gif",  "category": "Images", "is_default": false, "current_default": "eog.desktop"},
    {"mime": "image/webp", "category": "Images", "is_default": false, "current_default": null}
  ]
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

Deferred by design: a remote/community category database (the `Source` trait seam exists), a TUI front-end, fuzzy app-name matching, `[Added]`/`[Removed]` management, multi-app fallback lists, file locking, and bulk "set/unset everything unset" operations (composable today from the `--json` reads).

## License

No license chosen yet — add one (e.g. MIT or Apache-2.0) before distributing publicly.
