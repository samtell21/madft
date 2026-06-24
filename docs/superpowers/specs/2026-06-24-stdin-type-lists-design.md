# Piping type lists into `set` and `unset`

**Date:** 2026-06-24
**Status:** Approved, ready for planning

## Motivation

`madft set <app> [target] [--types …]` resolves a *target* (category path or
single mimetype) into an "umbrella" of types **via the category tree**, then
`--types` filters that umbrella. Types that are not in the tree (uncategorized,
e.g. an app's declared types whose `category == null`) can never enter the
umbrella, so:

```
madft set <app>            # uncategorized types aren't under root → nothing happens
```

Rather than grow a bespoke flag for every "operate on a computed set of types"
need, expose a **stdin contract**: a newline-delimited list of mimetypes that
becomes the operand set directly. This makes `set`/`unset` composable with
`jq`, `grep`, `sort`, etc. The motivating pipeline:

```
madft app <app> --json \
  | jq -r '.types[] | select(.category == null) | .mime' \
  | madft set <app> - --no-clobber
```

## Scope

- `set` and `unset` learn to read their type list from stdin.
- No other commands change. No new selection flags are added — selection is the
  pipeline's job.

## 1. Trigger

Both commands accept the type list from stdin two ways:

- **Explicit sentinel:** the positional target/mimetype is `-`.
  `madft set firefox - --no-clobber`, `madft unset -`.
- **Implicit:** no positional target **and** stdin is not a TTY.
  `… | madft set firefox --no-clobber`, `… | madft unset`.

A **real positional** target (`Media`, `image/png`) always wins; stdin is
ignored. No magic when the user has stated the operand explicitly.

For `unset`, the positional `mimetype` becomes optional. A TTY with no
positional and no `-` is an error (preserving today's required-arg protection).

## 2. Line protocol

Stdin is split on newlines. Each line is trimmed; blank/whitespace-only lines
are dropped. No comma-splitting (that is `--types`' job), no comment syntax, no
quote-stripping. A pure helper does this:

```rust
fn parse_type_lines(input: &str) -> Vec<String>
```

The contrast with `--types`: `--types a,b,c` is a *flag value*, comma-split
because shells dislike newlines in args; stdin is a *stream*, newline-split
because that is what Unix tools emit. Both land in a `Vec<String>` of types.

## 3. `set` semantics

When the type list comes from stdin, the lines are **raw mimetypes that become
the umbrella directly**, bypassing `resolve_umbrella` / `filter_umbrella`. This
is the whole point: uncategorized and inert types pass through, which is exactly
the `select(.category == null)` case.

Everything downstream is unchanged:

- Alias-canonicalized (`image/jpg` → `image/jpeg`), same as `--types`.
- App-declaration guard still applies → undeclared types go to `skipped_types`
  (`--force` overrides).
- `--no-clobber`, `--exact`, `--dry-run`, and provenance (`inherited_via`)
  behave exactly as today.
- `SetPlan.target` label is `(stdin)`, so JSON/human output records the source.

Garbage lines (no `/`, unknown types) are not special-cased: the
app-declaration guard skips them like any undeclared type. If *everything* is
skipped, the existing `app-handles-nothing-under-umbrella` error fires.

## 4. `unset` semantics

A list of mimetypes, each alias-canonicalized, each turned into an
`Edit::Unset`, written in one batch. No guards — removing an absent default is a
no-op, reported as such.

Output generalizes today's single-line result:

- **Human:** one line per type — `unset foo/bar` when a default was removed,
  `foo/bar: no user default to remove` when there was none. A single
  real-mimetype call is byte-identical to today.
- **JSON:** `{ "unset": [{ "mime": "…", "removed": true }, …], "removed_count": N }`.

The engine gains `unset_many(&[String]) -> Vec<(String, bool)>` (mime,
removed). The existing single `unset` becomes a thin wrapper over it (or callers
move to `unset_many`), preserving the current single-arg output.

## 5. Guard rails

- **`--types` + stdin → error** (`conflicting-type-source`). Both specify the
  operand set; combining is meaningless. (`unset` has no `--types`, so this only
  affects `set`.)
- **Empty stdin** (piped but zero types after cleanup) → **error**, never a
  silent fall-through. The critical safety case: auto-detect must never turn an
  empty pipe into "set `<app>` as default for the entire tree" or an
  unset-nothing no-op masquerading as success.

## 6. Structure & testing

- `execute()` cannot currently see stdin. Thread an **injectable reader** so
  tests pass a string/`&[u8]` instead of real stdin — no TTY/process mocking.
  `run()` wires the real `std::io::stdin()` and its TTY status.
- TTY detection is needed only for the *implicit* trigger; the explicit `-`
  sentinel needs none. Keep TTY status as an input to the resolution helper so
  tests drive both branches deterministically.
- One shared helper resolves the type list from *positional-`-`-or-piped-stdin*
  for both commands; `parse_type_lines` is unit-tested directly.
- New tests cover: `-` sentinel, implicit piped stdin, real-target-beats-stdin,
  empty-stdin error, `--types`+stdin conflict, whitespace/blank-line cleanup,
  alias canonicalization through the stdin path, `set` skip/force on undeclared
  piped types, and `unset` per-type removed/not-removed reporting.

## Out of scope

- Reading type lists from a file path (the pipeline can `cat file | …`).
- stdin for any command other than `set` / `unset`.
- New selection/filtering flags — composition replaces them.
