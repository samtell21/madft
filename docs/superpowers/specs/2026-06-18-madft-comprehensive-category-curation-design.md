# madft v0.4.0 — comprehensive category curation

Status: approved design (pending spec review). Data-only; builds on v0.3.0.

## Problem

The category tree decides which mimetypes have a curated home. Today it places ~152
of the 912 shared-mime-info types; the other **760 fall into the flat `Other` node**.
Worse, the type *universe* madft sees comes only from shared-mime-info, so types that
**libmagic (`file`) emits but shared-mime-info never declares are invisible to madft
entirely** — there are a few hundred of these.

The user hit the canonical case with fonts. A `.ttf` is `font/ttf` to `gio` (glob match)
but `font/sfnt` to `file`/`xdg-open` (content sniff at the generic SFNT supertype).
`font/sfnt` is **not in shared-mime-info at all** (it lives only in libmagic's
`/usr/share/misc/magic`), so it was neither in the `Fonts` category nor anywhere in
madft's universe. Result: `madft set org.fontforge.FontForge Fonts -a --force` set the
7 freedesktop font types but **missed `font/sfnt`**, so opening the file from a
content-sniffing opener (yazi → `xdg-open`) still failed. The user had to discover
`font/sfnt` by hand and force-assign it — exactly the investigation they don't want
to repeat for the next format.

## Key finding: no code change is needed

The curated tree is already the **source of truth for the taxonomy**, not a mere view
over freedesktop:

- `categories/source.rs` does **not** validate listed types against the MIME universe.
- `categories/merge.rs` places listed types **unconditionally** (the universe is only
  consulted to sweep *unplaced* types into `Other`).
- A handler-less type is "inert" and hidden by default, but `-a` reveals it and
  `--force` sets it past the declares-guard.

So a libmagic-only type like `font/sfnt`, once **listed in `categories.toml`**, becomes
a real, settable leaf. Proven end-to-end: adding `font/sfnt` to the live
`~/.local/share/madft/categories.toml` made `set … Fonts -a --force` go 7 → 8 types,
including `font/sfnt`.

This work is therefore **pure data curation** of `data/categories.toml`. No Rust changes.

## Approach: principled desktop-relevant expansion

Pull every type a person would plausibly open into a category, and add the libmagic-only
sniff types (the `font/sfnt` class). Two approaches were rejected:

- **Exhaustive "categorize all ~900+".** Most of the tail is instrument/scientific
  formats (`application/grib;edition=1`, `application/bufr`, `application/netcdf`) that
  no handler app opens. Forcing them into categories is noise; `Other` exists for this.
- **Auto-import libmagic at runtime.** Fragile (parsing `/usr/share/misc/magic`,
  inconsistent type mapping) and fights madft's deterministic, human-curated design.

### Inclusion rubric (keeps curation honest)

A type earns a category **iff** a user could plausibly want a **default handler app
(terminal *or* GUI)** to open it, **and** it is a recognizable real-world format.
Terminal apps count: `nvim.desktop` is `Terminal=true` and is a legitimate default —
categories are for *any* type that might want a default, not just GUI-openable ones.

**Excluded → stays in `Other`:** scientific/instrument formats; raw executables and
byte-code (no meaningful "default opener"); transport/internal/protocol types;
parameterized junk (`;edition=1`, odd casing like `application/CDFV2`); and legacy
aliases that canonicalize onto a type already placed (folded automatically by the MIME
DB, so listing them is redundant).

### Two type sources, reconciled

1. **shared-mime-info `Other`** (760 types) — lift the rubric-passing ones into categories.
   Enumerate via `madft types Other -a`.
2. **libmagic `!:mime` set minus shared-mime-info** — the invisible `font/sfnt` class.
   Enumerate via `grep -rhoE '!:mime\s+\S+' /usr/share/misc/magic*` set-diffed against
   `/usr/share/mime/types`. These can *only* enter the universe through `categories.toml`.

## Category changes

Existing categories absorb most of the lift (`Text.Development.*`, `Archives.*`,
`Media.*`, `Documents.*`, `Images.*`). Proposed **new** top-level/sub categories
(exact membership finalized during planning):

- **`Fonts`** (expand): add `font/sfnt` plus other real font types both detectors emit —
  `application/vnd.ms-fontobject` (EOT), `application/x-font-bdf`,
  `application/x-font-linux-psf`, `application/x-font-snf`, `application/x-font-speedo`,
  `application/x-font-ttx`, etc.
- **`Models`** — 3D/CAD: `model/stl`, `model/obj`, `model/gltf+json`,
  `model/gltf-binary`, `model/3mf`, `model/vrml`, `model/iges`, `model/mtl`, …
- **`Data`** — `application/vnd.sqlite3`, `application/x-sqlite{2,3}`,
  `application/geopackage+sqlite3`, and the data `+json`/`+yaml` family
  (`application/geo+json`, `application/ld+json`, `application/schema+json`,
  `application/json-patch+json`, `application/raml+yaml`, …).
- **`Software`** — installable/runnable bundles: `application/x-iso9660-appimage`,
  `application/vnd.flatpak.ref`, `application/vnd.flatpak.repo`, snap, `application/wasm`, …
  (distinct from `Archives.Packages`, which is distro packages.)
- **Calendar & contacts** — a small home for `text/calendar` and `text/vcard` (placement
  finalized in plan: a new top-level `Personal` or under `Documents`).

Single-placement (each type exactly one home) and valid category names are invariants
the merge enforces; curation must respect them.

## Sync & execution

- Edit the built-in `data/categories.toml`. The user's live
  `~/.local/share/madft/categories.toml` is **byte-identical** to it today, so regenerate
  it with `madft init --force` after the build — no personal edits to clobber.
- **Subagent-driven** execution (user's preferred style): fan out subagents over slices
  of the `Other` list and the libmagic-only list, each proposing rubric-filtered
  placements with a one-line justification per type; the main thread reconciles,
  resolves single-placement conflicts, writes the file, and verifies.

## Acceptance criteria

- `cargo build` clean; **all tests pass**, especially `default_categories_is_valid`
  (built-in parses, no duplicate placements) and `tree_is_total_over_all_types`.
- No duplicate placements; all category names valid.
- `font/sfnt` is in `Fonts`; `madft set org.fontforge.FontForge Fonts -a --force`
  includes it.
- `Other` shrinks substantially without forcing junk — target **~760 → ≤ 400**.
- Spot-check: run `file --mime-type -b` on several real sample files (a `.ttf`, plus a
  handful across new categories) and confirm `madft info <type>` reports a sensible
  category.
- Version bump to `0.4.0`; README/docs note describing the broadened taxonomy and the
  "libmagic-only types are first-classed via the curated tree" behavior, per existing
  doc conventions.

## Out of scope (YAGNI)

- Any Rust/engine change. This is data only.
- Auto-importing or shelling out to libmagic.
- Curating the scientific/instrument long tail.
- The terminal-vs-GUI dispatch problem (`gio`/`xdg-open` ignoring `Terminal=true`) —
  a separate investigation the user will pursue elsewhere.
