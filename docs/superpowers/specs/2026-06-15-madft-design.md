# madft (mimeapps default) — Design Spec

- **Date:** 2026-06-15
- **Status:** Approved design; pre-implementation.
- **Language:** Rust (binary + internal library).

## 1. Purpose & scope

`madft` is a CLI for **inspecting and setting XDG default applications**, organized around a
human-curated **category tree** (e.g. `Media.Video → video/mp4`). It is a *default-setter*,
not a capability-explorer — "what could open this type" is already answered by `gio`/`xdg-mime`.

A future TUI front-end (via the `ptui` framework) will shell out to this CLI and parse its
`--json` output. That front-end is **out of scope** here; the CLI is designed to be the stable
machine-facing API it will wire to.

### Non-goals (explicit scope boundaries)
- Not a "what can this app do" tool.
- **Inheritance (the subclass DAG) is read-only annotation, never a set target.** You cannot,
  in normal operation, set an app as default for a type the app does not *explicitly* declare.
- Core logic never uses subclass relationships to decide whether an app "handles" a type.

## 2. Core concepts & invariants

- **Two distinct trees, never conflated:**
  1. **Curated category tree** — a human navigation overlay (`Media.Video`). Pure UX, authored.
  2. **Freedesktop subclass DAG** — the real inheritance (`text/html → text/plain`; a DAG, not a
     tree: multiple parents possible; plus *aliases* like `image/jpg → image/jpeg`). Surfaced as
     read-only "what you'd inherit if unset" annotation.
- **Family metaphors are kept disjoint across the two trees** to prevent namespace confusion: the
  subclass DAG uses `supertypes` / `ancestor_types` (mimetype-domain); the category tree uses
  `subcategories` / `parent` (node-domain). No family word is shared between the two.
- **"App X handles type T" = exact `MimeType=` declaration** in X's desktop file. This governs
  both the applicable-apps query and the block-on-set guard. Never subclass, never inheritance.
- **Notation (three delimiters, three meanings):**
  - `.` — category hierarchy: `Media`, `Media.Video`, `Other`
  - `:` — boundary between category-space and mimetype-space
  - `/` — the mimetype's own internal delimiter (untouched)
  - Fully-qualified reference: `Media.Video:video/mp4`, `Other:application/x-new`.
  - Category names are constrained to `[A-Za-z0-9 _-]` (no `.`, `:`, `/`), enforced at load.
- **The category tree is total over `mimedb.all_types()`** — every registered type resolves to
  some node; nothing is ever invisible. Unplaced types fall to a flat `Other` node and display as
  `Other:<mimetype>` (the prefix is already visible in the mimetype, so no `Other.<prefix>` sub-bucket).
- **Writes touch only `~/.config/mimeapps.list` `[Default Applications]`** — never system files,
  never `[Added]`/`[Removed]`.
- **Correct XDG precedence on reads** (user `~/.local/share` shadows system) — explicitly NOT the
  inverted first-seen behavior observed in wofi.

## 3. Architecture & modules

Single Rust binary; an internal library holds the engine + facts so it is testable independently
of the CLI and reusable by ptui later (shell-out now; possible direct linking later).

| Module | Job | Reads | Key API |
|---|---|---|---|
| `mimedb` | freedesktop MIME facts (subclass DAG) | `…/mime/{types,subclasses,aliases}` (user + system) | `all_types()`, `canonicalize(alias)`, `supertypes(t)` (direct subclass parents), `ancestor_types(t)` (transitive closure = inherit-if-unset chain), `comment(t)` (best-effort, lazy) |
| `appindex` | exact-declaration authority | one pass over `$XDG_DATA_HOME:$XDG_DATA_DIRS` `/applications/*.desktop` (`Name`, `NoDisplay`, `MimeType=`) | `apps_for_type(t)`, `declares(id,t)`, `app(id)` |
| `defaults` | effective current default | the `mimeapps.list` precedence chain | `current_default(t) -> Option<DesktopId>` |
| `categories` | layered category tree | `Source` trait → `FileSource{defaults, overrides}` (future `RemoteSource`) | `tree()`, `types_under(id)`, `node_by_path(path)`, `path(id)` |
| `writer` | mutate user defaults | — | pure `apply(content, edits) -> content`; IO wrapper adds atomic-replace + `.bak` |
| `engine` | orchestrate the above into operations | — | `ls`, `types`, `info`, `apps`, `set`, `unset`, `get` |
| `cli` | clap subcommands; human vs `--json` render | — | — |

### Core types
- `MimeType(String)` — always alias-canonicalized.
- `DesktopId(String)` — desktop-file basename (`mpv.desktop`).
- `App { id: DesktopId, name: String, nodisplay: bool, mimetypes: Set<MimeType> }`.
- Category tree — **arena / index model** (explicit parent links, no stored dotted path):
  - `CategoryId(usize)` — a handle into the arena.
  - `CategoryNode { name: String, parent: Option<CategoryId>, subcategories: Vec<CategoryId>, types: Vec<MimeType> }` — `types` is present on **every** node (interior nodes can own types directly, not just leaves).
  - `CategoryTree { arena: Vec<CategoryNode>, root: CategoryId }`, exposing `path(id) -> String` (derived by walking `parent`, names joined by `.`) and `node_by_path(&str) -> Option<CategoryId>` (for CLI path args). The dotted path is always *computed*, never stored.
- `TypeInfo { mime, comment: Option<String>, current_default: Option<DesktopId>, applicable_count: usize, ancestor_types: Vec<MimeType> }`.

## 4. Category model (the layered merge)

Resolution builds the tree from the `defaults` source, applies `overrides`, then sweeps unplaced
types into `Other`:

```
tree = defaults                       # maintained/shared placements
       ← overrides                    # local re-placements + filing of unlisted types (wins)
       ← Other:<type>                 # any mimedb type still unplaced (flat catch-all)
```

Precedence per type: **override > default > Other**. **A mimetype has exactly one direct
placement** — this is enforced:
- Within a single source file, a type appearing under two category paths is a load error
  (`DuplicatePlacement`).
- Across layers, an `overrides` placement *supersedes* the `defaults` placement (this is the
  intended re-placement mechanism, not a conflict).
- After the merge, the type's single home is: its `overrides` placement if any, else its `defaults`
  placement, else `Other`.

Umbrella membership is distinct from direct placement: a type directly placed in `Media.Video` is
also "under" `Media`. `types_under(node)` returns the recursive union over a node and its
`subcategories`.

The `Source` trait abstracts "load the default tree" so the file source can later be swapped for a
remote, community-maintained DB (which would fetch + cache into the same data-dir file). MVP ships
the file source only; the trait seam is the only remote-readiness required now.

### File locations & format
- `defaults` ← `${XDG_DATA_HOME:-~/.local/share}/madft/categories.toml` (maintained/shared; future
  remote cache target).
- `overrides` ← `${XDG_CONFIG_HOME:-~/.config}/madft/overrides.toml` (personal).

Both use the same TOML grammar — a table per dotted category path mapping to an ordered type list:

```toml
# categories.toml (defaults) — and overrides.toml (same grammar)
["Media"]
types = ["application/ogg"]      # interior nodes may own types directly, not just leaves

["Media.Video"]
types = ["video/mp4", "video/x-matroska", "video/webm"]

["Media.Audio"]
types = ["audio/mpeg", "audio/flac", "audio/ogg"]

["Documents"]
types = ["application/pdf", "application/epub+zip"]

["Images"]
types = ["image/png", "image/jpeg", "image/gif", "image/webp"]
```

Hierarchy is implied by dotted keys (`Media` is the parent of `Media.Video`), and a node may carry
its own `types` at any level. An override file listing a type under a different node moves it there
(supersedes the default placement); new category paths simply appear. A type listed under two paths
within the same file is a load error (single-placement, see §2).

## 5. Command surface

clap-based. Every command supports human output (default) and `--json` (the ptui contract).

| Command | Behavior |
|---|---|
| `madft ls [PATH]` | children categories + leaf types at a node (root if no PATH); each leaf annotated with current default + applicable-app count |
| `madft types <PATH>` | all mimetypes under the umbrella, recursive + canonicalized |
| `madft info <mimetype>` | canonical name (+ aliases), comment, current default, `ancestor_types` (inherit-if-unset chain), applicable apps (exact-decl) |
| `madft apps <PATH\|mimetype>` | applicable apps under the umbrella; per app: which & how many of the umbrella's types it declares, sorted by coverage |
| `madft set <PATH\|mimetype> <app> [--types t1,t2,…] [--dry-run]` | set `<app>` default for the umbrella's types it declares; `--types` restricts to a subset; **guards** if app declares none; `--dry-run` prints the plan without writing |
| `madft unset <mimetype>` | remove a user default for the type |
| `madft get <mimetype>` | print the bare current default (scriptable) |

**Behavior details:**
- `<app>` accepts a desktop-id with optional `.desktop` (`mpv` ≡ `mpv.desktop`).
- `set` at an umbrella applies to exactly the umbrella's declared types and **reports the skipped,
  unhandled types as informational output, not an error** (e.g. `set Media mpv` sets video/audio,
  reports images skipped).
- Exit `0` on success; non-zero on guard failure / unknown path / unknown app. `--json` queries
  emit a stable schema; errors emit `{"error": {"kind": "...", "message": "..."}}`.

## 6. Write safety

- Parse the existing `mimeapps.list` preserving all sections, keys, ordering, and comments where
  feasible; unknown sections/keys round-trip verbatim.
- Edit = upsert/remove keys within `[Default Applications]` only.
- **Atomic:** write a temp file in the same directory → fsync → rename over the target.
- **Backup:** copy to `mimeapps.list.bak` before writing.
- **Idempotent:** setting an existing value is a no-op write-wise.
- Create a minimal file (`[Default Applications]` only) if none exists. Never create
  `[Added Associations]` / `[Removed Associations]`.
- Single-user, last-writer-wins; file locking is deferred (YAGNI).

## 7. Error model

Typed errors (`thiserror`): `UnknownPath`, `UnknownApp`, `AppHandlesNothingUnderUmbrella`
(the block-on-set guard), `InvalidCategoryName`, `DuplicatePlacement` (a type placed under two
categories within one source file), `MimeDbNotFound`, `Io`, `Parse`.

- Human → clear stderr message + non-zero exit; `--json` → `{"error": {kind, message}}`.
- **Partial coverage is success, not error** (the mpv-in-Media case).
- One malformed desktop/mimeapps line is skipped with a warning — never a crash.
- A missing MIME DB (`/usr/share/mime` absent) is fatal (`MimeDbNotFound`).

## 8. Testing strategy

- Every reader takes an **injectable root-path set**, so all tests run against **fixture XDG trees**
  with zero reliance on the host system.
- **Unit:** alias canonicalization; transitive multi-parent `ancestors`; category-merge precedence
  and **totality**; appindex inversion + exact-decl guard; writer round-trip (preserve unrelated
  sections, upsert/unset, idempotency, atomic temp+rename).
- **Golden/integration:** a fixture tree (`mime/`, `applications/`, `mimeapps.list`,
  `categories.toml`, `overrides.toml`) → run engine ops → assert `--json`. The **mpv-in-Media**
  scenario is a named test (sets video/audio, reports images skipped, writes nothing for images).
- No network; `RemoteSource` is not exercised in MVP.

## 9. MVP boundary (YAGNI)

**In:** the 7 commands; file-backed `defaults` + `overrides`; exact-declaration semantics;
`ancestors` annotation; atomic backed-up writes to `~/.config/mimeapps.list`; human + `--json`.

**Deferred (seams kept, not built):** remote/community category DB (`Source` trait only);
ptui/TUI wiring; fuzzy app-name matching; `[Added]`/`[Removed]` management; multi-app fallback
lists in a single default; file locking. Per-type `comment` is best-effort (lazy; omitted if absent).
