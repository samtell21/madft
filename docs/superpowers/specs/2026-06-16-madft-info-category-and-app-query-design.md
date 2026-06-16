# madft — `info` category, `app` query, and `set --force` (Design Addendum)

- **Date:** 2026-06-16
- **Status:** Approved design; pre-implementation.
- **Amends:** `docs/superpowers/specs/2026-06-15-madft-design.md` (the MVP spec). Everything there still holds except where this document explicitly overrides it.
- **Built on:** the completed MVP (Plans 1–4). This is a post-MVP increment ("Plan 5").

## Motivation

Two TUI-facing gaps and one escape hatch, requested after dogfooding the MVP:
1. A type's `info` doesn't say *which category it lives in* — the navigation breadcrumb.
2. There's no **app-centric** view: given an app, what does it declare and what is it currently the default for? (`apps` answers the inverse — who can handle an umbrella.)
3. `set` is strictly exact-declaration; there is no way to override it for the occasional "I know what I'm doing" case (e.g. make `nvim` the default for `text/css`).

## 1. `category` in `info`

`TypeInfo` gains a field:

```rust
pub category: Option<String>,   // dotted category path, e.g. "Media.Video" or "Other"
```

- It is the dotted path of the node the (canonicalized) type is **directly** placed in. The single-placement invariant (MVP §2/§4) guarantees at most one.
- `None` only when the type is not in the system MIME universe at all (e.g. a typo'd query) — i.e. it appears in no node, not even `Other`. (Every real type is total over the tree, so for installed types this is always `Some`.)
- **Format:** dotted `String`, consistent with `ls`'s `subcategories` (which are already dotted). JSON emits the string (or `null`); human renders a `category: Media.Video` line.
- **Engine:** `info` sets `category: self.tree.category_of(&canon).map(|id| self.tree.path(id))`.

New `CategoryTree` accessor:

```rust
/// The id of the node that DIRECTLY places `t` (its single home), if any.
pub fn category_of(&self, t: &MimeType) -> Option<CategoryId>;
```

Implementation: scan the arena for the node whose `types` contains `t` (single-placement ⇒ at most one match). O(total placed types) per call — negligible at tree scale. Callers pass an already-canonical `MimeType`.

## 2. `madft app <id>` — app-centric view

A new command and engine operation: given a desktop-id, list its declared mimetypes, the category each falls in, and whether the app is currently the default for it.

**CLI:** `App { id: String }`. `<id>` accepts the optional `.desktop` suffix (`swayimg` ≡ `swayimg.desktop`), like `set`/`get`. An app not present in the index → `UnknownApp` (exit 1), consistent with `set`.

**Engine:** `pub fn app(&self, id: &str) -> Result<AppReport>`.

**Result structs (derive `Serialize, Debug`):**

```rust
pub struct AppReport {
    pub id: String,
    pub name: String,
    pub declares: usize,          // count of distinct (canonical) declared types
    pub default_for: usize,       // how many of them this app is currently the default for
    pub types: Vec<AppTypeRow>,
}

pub struct AppTypeRow {
    pub mime: String,             // canonical
    pub category: Option<String>, // dotted path (same shape as TypeInfo.category)
    pub is_default: bool,         // is THIS app the current default for `mime`?
    pub current_default: Option<String>, // the actual current default id (may be this app, another, or null)
}
```

**Semantics:**
- Declared types come from the app's `MimeType=` set (the `AppIndex` `App.mimetypes`). Each is alias-canonicalized; duplicates that collapse to the same canonical type are de-duped. `declares == types.len()`.
- Per type: `current_default = defaults.current_default(canon)`; `is_default = current_default == Some(this id)`; `category = tree.category_of(canon).map(path)`.
- `default_for` = count of rows where `is_default`.
- **Ordering (deterministic):** `is_default` rows first, then alphabetical by `mime`. (No reliance on `apps_for_type`/read-dir order.)

**JSON example** (`madft app swayimg --json`):
```jsonc
{
  "id": "swayimg.desktop",
  "name": "swayimg",
  "declares": 5,
  "default_for": 2,
  "types": [
    {"mime":"image/png",     "category":"Images", "is_default":true,  "current_default":"swayimg.desktop"},
    {"mime":"image/jpeg",    "category":"Images", "is_default":true,  "current_default":"swayimg.desktop"},
    {"mime":"image/gif",     "category":"Images", "is_default":false, "current_default":"eog.desktop"},
    {"mime":"image/webp",    "category":"Images", "is_default":false, "current_default":null},
    {"mime":"image/svg+xml", "category":"Images", "is_default":false, "current_default":"org.mozilla.firefox.desktop"}
  ]
}
```

**Human form:**
```
swayimg.desktop (swayimg) — declares 5 types, default for 2:
  ★ image/png      [Images]  (default: swayimg.desktop)
  ★ image/jpeg     [Images]  (default: swayimg.desktop)
    image/gif      [Images]  (default: eog.desktop)
    image/webp     [Images]  (default: —)
    image/svg+xml  [Images]  (default: org.mozilla.firefox.desktop)
```
(`★` marks rows where `is_default`; `[<category>]` shows the dotted path, or `[—]` if `None`; `(default: —)` when unset.)

## 3. `set --force` / `-f`

`set` gains a force flag that overrides the **block-on-set guard** (MVP §2/§7's exact-declaration requirement). This is the documented escape hatch the MVP §1 hinted at ("in normal operation").

- **CLI:** `Set { target, app, types: Vec<String>, force: bool, dry_run: bool }` with `#[arg(short = 'f', long)] force`.
- **Engine signature change:**
  ```rust
  pub fn set(&self, target: &str, app: &str, types_filter: Option<&[String]>, force: bool, dry_run: bool) -> Result<SetPlan>;
  ```
- **Partition rule:** a targeted umbrella type goes to `set_types` if `force || app.declares(t)`, else to `skipped_types`. So under `--force`, nothing is rejected and `skipped_types` is empty.
- **Guard:** `AppHandlesNothingUnderUmbrella` fires only when `set_types` ends up empty. Without `--force` this happens when the app declares none of the umbrella's types (unchanged MVP behavior). With `--force` over a non-empty umbrella, `set_types` is non-empty, so the guard does not fire.
- `--force` overrides the *declaration* guard only. An app not present in the index still errors with `UnknownApp` — you cannot set a default to an app that isn't there.
- **`SetPlan` gains** `pub forced: bool` so `--json` consumers can see the set was forced.
- **CLI UX:** when the guard fires *without* `--force`, the human error message appends a hint: `(use --force to override)`. (The `Error` value/`kind` are unchanged; the hint is added only in the CLI human-error render, not the JSON envelope.)

Example: `madft set text/css nvim` → rejected (`app-handles-nothing-under-umbrella`, hint to use `--force`). `madft set text/css nvim --force` → sets it; `SetPlan.set_types == ["text/css"]`, `skipped_types == []`, `forced == true`.

## Amendments to the MVP spec (`2026-06-15-madft-design.md`)

- **§2** — the exact-declaration block-on-set guard is overridable via `set --force`; the default (strict) behavior is unchanged. Reads (`apps`, `info`, the `apps_for_type` query) remain exact-declaration and are NOT affected by `--force`.
- **§3 / core types** — `TypeInfo` gains `category: Option<String>`. New result types `AppReport` / `AppTypeRow`. New `CategoryTree::category_of`.
- **§5 / command surface** — new `madft app <id>` row; `set` gains `--force`/`-f`. `set`'s `--json` output (`SetPlan`) gains `forced`.
- **§7** — no new error variants. `set --force` suppresses `AppHandlesNothingUnderUmbrella` by changing the partition (not by catching the error).

## Testing

- **Unit:** `category_of` (places a type to the right node; `None` for an out-of-universe type) in `tree.rs`. `info` now asserts `category`. `engine::app` (declared set, `is_default`/`current_default`/`category` per row, ordering, `default_for` count, `UnknownApp`). `set` force-path (forces a non-declared type; `forced`/`set_types`/`skipped_types`; guard still fires on an empty umbrella; `UnknownApp` still errors under `--force`).
- **Golden (`--json`):** `madft app mpv --json` over the engine fixture (mpv declares video/audio; assert rows, categories from the engine `categories.toml`, `default_for`); `madft info video/mp4 --json` asserts `category == "Media.Video"`; `madft set text/css nvim --force --json` asserts `forced == true` and the type set (using a writable temp config, or `--dry-run` for the partition assertion).
- **Determinism:** `app.types` ordered (`is_default` desc, then `mime`); `category` is the deterministic tree path. No new read-dir surface.

## Scope / non-goals

- No new dependencies. Additive `--json` fields only (`category`, `forced`) — existing consumers don't break.
- Still deferred (unchanged from MVP §9): `comment(t)`, reverse alias listing, `RemoteSource`, ptui wiring, fuzzy app-name matching, `[Added]`/`[Removed]` management, file locking.
- `--force` does not enable subclass/inheritance-based setting; it only waives the exact-declaration check for the explicitly targeted types. Inheritance remains read-only annotation.
