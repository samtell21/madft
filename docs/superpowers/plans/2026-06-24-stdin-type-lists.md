# Piping type lists into `set` and `unset` — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let `madft set` and `madft unset` read their mimetype operand list from stdin (newline-delimited), triggered by a `-` target sentinel or by piped (non-TTY) stdin, so uncategorized types become composable via `jq`/`grep`/etc.

**Architecture:** Stdin lines become a raw mimetype list that bypasses category-tree resolution and becomes the operand set directly; all existing downstream `set` guards (`--force`, `--no-clobber`, `--exact`, declaration check) and `unset` semantics are reused unchanged. The CLI core gains an injectable stdin reader + TTY flag so the behavior is unit-testable with a byte slice, never a real terminal.

**Tech Stack:** Rust, clap (derive), serde/serde_json, thiserror. TTY detection via `std::io::IsTerminal` (std, no new dependency).

## Global Constraints

- No new crate dependencies — TTY detection uses `std::io::IsTerminal` (stable std).
- Single-argument `unset <mimetype>` output (human **and** JSON) stays byte-identical to today; the new list shape applies only to the stdin path.
- `set`'s existing behavior for a real target or no-target-on-a-TTY (whole-tree root) is unchanged.
- Stable kebab-case error kinds for the `--json` envelope; every new `Error` variant gets an `error_kind` mapping.
- Edits touch only `[Default Applications]`; reuse `writer::write_user_defaults` (atomic + `.bak`).

---

### Task 1: New error variants + kinds

**Files:**
- Modify: `src/error.rs` (enum + a display test)
- Modify: `src/cli.rs:100-111` (`error_kind`)

**Interfaces:**
- Produces: `Error::ConflictingTypeSource`, `Error::EmptyTypeList`, `Error::MissingMimetype`; `error_kind` maps them to `"conflicting-type-source"`, `"empty-type-list"`, `"missing-mimetype"`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/error.rs`:

```rust
    #[test]
    fn new_stdin_variants_display() {
        assert_eq!(
            Error::ConflictingTypeSource.to_string(),
            "--types cannot be combined with a stdin type list"
        );
        assert_eq!(Error::EmptyTypeList.to_string(), "no mimetypes on stdin");
        assert_eq!(
            Error::MissingMimetype.to_string(),
            "no mimetype given (provide one, pipe a list, or use '-')"
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib error:: 2>&1 | tail -20`
Expected: FAIL — `no variant named ConflictingTypeSource` (compile error).

- [ ] **Step 3: Add the variants**

In `src/error.rs`, inside `pub enum Error`, after the `Parse { .. }` variant:

```rust
    #[error("--types cannot be combined with a stdin type list")]
    ConflictingTypeSource,

    #[error("no mimetypes on stdin")]
    EmptyTypeList,

    #[error("no mimetype given (provide one, pipe a list, or use '-')")]
    MissingMimetype,
```

- [ ] **Step 4: Map the kinds**

In `src/cli.rs`, in `fn error_kind`, add three arms before `Error::Io(_)`:

```rust
        Error::ConflictingTypeSource => "conflicting-type-source",
        Error::EmptyTypeList => "empty-type-list",
        Error::MissingMimetype => "missing-mimetype",
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --lib error:: 2>&1 | tail -20`
Expected: PASS. Also `cargo build 2>&1 | tail -5` — no non-exhaustive-match warnings in `error_kind`.

- [ ] **Step 6: Commit**

```bash
git add src/error.rs src/cli.rs
git commit -m "feat(error): add stdin type-list error variants and kinds"
```

---

### Task 2: `parse_type_lines` pure helper

**Files:**
- Modify: `src/cli.rs` (add free function near the top, after `to_json`)
- Test: `src/cli.rs` `tests` module

**Interfaces:**
- Produces: `fn parse_type_lines(input: &str) -> Vec<String>` — splits on newlines, trims each line, drops blank/whitespace-only lines. No comma-splitting, no comment handling, no quote-stripping.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/cli.rs`:

```rust
    #[test]
    fn parse_type_lines_trims_and_skips_blanks() {
        let input = "  text/x-foo \n\nimage/png\n   \napplication/pdf\n";
        assert_eq!(
            parse_type_lines(input),
            vec!["text/x-foo", "image/png", "application/pdf"]
        );
    }

    #[test]
    fn parse_type_lines_does_not_split_on_commas() {
        // Unlike --types, a comma is part of the (here nonsensical) line, not a delimiter.
        assert_eq!(parse_type_lines("a/b,c/d\n"), vec!["a/b,c/d"]);
    }

    #[test]
    fn parse_type_lines_empty_input_is_empty() {
        assert_eq!(parse_type_lines("   \n\n").len(), 0);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib parse_type_lines 2>&1 | tail -20`
Expected: FAIL — `cannot find function parse_type_lines`.

- [ ] **Step 3: Implement the helper**

In `src/cli.rs`, after `fn to_json(...)`:

```rust
/// Parse a newline-delimited mimetype list from stdin. Each line is trimmed;
/// blank/whitespace-only lines are dropped. Unlike `--types` (comma-split flag
/// value), stdin is a stream split on newlines — no comma/comment/quote magic.
fn parse_type_lines(input: &str) -> Vec<String> {
    input
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib parse_type_lines 2>&1 | tail -20`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): parse_type_lines stdin parser"
```

---

### Task 3: Engine `set_types` (explicit-umbrella set)

**Files:**
- Modify: `src/engine.rs:724-809` (extract a private core; add `set_types`)
- Test: `src/engine.rs` `write_tests` module

**Interfaces:**
- Consumes: existing `SetOptions`, `SetPlan`, `self.mimedb.canonicalize`, `self.appindex`, `self.handles`, `self.nearest_declared_ancestor`.
- Produces: `pub fn set_types(&self, app: &str, types: &[String], opts: SetOptions) -> Result<SetPlan>` — canonicalizes & de-dupes `types`, uses them as the umbrella directly (bypassing the tree), label `"(stdin)"`, no `--types` filter. Same guards/partitioning as `set`.

- [ ] **Step 1: Write the failing test**

The read-only fixture engine has `mpv` declaring `video/mp4`, `audio/mpeg`, `video/x-matroska` (see existing `set_dry_run_partitions_without_writing`). Add to `write_tests`:

```rust
    #[test]
    fn set_types_uses_explicit_list_bypassing_tree() {
        let e = read_only_engine();
        // application/x-not-in-any-category is not in the tree, but mpv won't
        // declare it; video/mp4 is declared. Declared ones become set_types,
        // undeclared ones are skipped — same partition rules as `set`.
        let list = vec!["video/mp4".to_string(), "application/x-uncategorized".to_string()];
        let plan = e
            .set_types("mpv", &list, SetOptions { dry_run: true, ..Default::default() })
            .unwrap();
        assert_eq!(plan.target, "(stdin)");
        assert_eq!(plan.set_types, vec!["video/mp4"]);
        assert_eq!(plan.skipped_types, vec!["application/x-uncategorized"]);
        assert!(!plan.written);
    }

    #[test]
    fn set_types_canonicalizes_and_dedupes() {
        let e = read_only_engine();
        // image/jpg is an alias of image/jpeg; force so declaration doesn't gate it.
        let list = vec!["image/jpg".to_string(), "image/jpeg".to_string()];
        let plan = e
            .set_types("mpv", &list, SetOptions { force: true, dry_run: true, ..Default::default() })
            .unwrap();
        assert_eq!(plan.set_types, vec!["image/jpeg"]); // deduped to one canonical
    }

    #[test]
    fn set_types_empty_candidates_errors() {
        let e = read_only_engine();
        let list = vec!["application/x-uncategorized".to_string()];
        let err = e
            .set_types("mpv", &list, SetOptions { dry_run: true, ..Default::default() })
            .unwrap_err();
        assert!(matches!(err, Error::AppHandlesNothingUnderUmbrella { .. }));
    }
```

> If `read_only_engine()` is not the helper name in `write_tests`, use the same constructor the neighboring `set_*` tests use (e.g. the one in `set_relaxed_guard_matches_via_inheritance`).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib set_types 2>&1 | tail -20`
Expected: FAIL — `no method named set_types`.

- [ ] **Step 3: Refactor `set` to extract a core, add `set_types`**

In `src/engine.rs`, replace the body of `pub fn set` (lines ~731-808, everything after the doc comment) so that the post-umbrella logic lives in a private `set_core`, and `set` calls it:

```rust
    pub fn set(
        &self,
        app: &str,
        target: Option<&str>,
        types_filter: Option<&[String]>,
        opts: SetOptions,
    ) -> Result<SetPlan> {
        let (label, raw) = self.resolve_umbrella(target)?;
        let umbrella = self.filter_umbrella(target, raw, opts.show_all, types_filter.is_some());
        let filter: Option<Vec<MimeType>> = types_filter.map(|fs| {
            fs.iter()
                .map(|s| self.mimedb.canonicalize(&MimeType::new(s.as_str())))
                .collect()
        });
        self.set_core(&DesktopId::new(app), label, umbrella, filter, opts)
    }

    /// `set` over an explicit, already-known list of mimetypes (e.g. from stdin):
    /// the types become the umbrella directly, bypassing the category tree. They
    /// are alias-canonicalized and de-duplicated (first occurrence wins). Label is
    /// `"(stdin)"`. No `--types` filter applies. All other guards match `set`.
    pub fn set_types(&self, app: &str, types: &[String], opts: SetOptions) -> Result<SetPlan> {
        let mut seen: std::collections::HashSet<MimeType> = std::collections::HashSet::new();
        let umbrella: Vec<MimeType> = types
            .iter()
            .map(|s| self.mimedb.canonicalize(&MimeType::new(s.as_str())))
            .filter(|t| seen.insert(t.clone()))
            .collect();
        self.set_core(&DesktopId::new(app), "(stdin)".to_string(), umbrella, None, opts)
    }

    /// Shared core of `set`/`set_types`: given a resolved umbrella, partition into
    /// set/skipped/unchanged, compute provenance, and write (unless dry-run).
    fn set_core(
        &self,
        app_id: &DesktopId,
        label: String,
        umbrella: Vec<MimeType>,
        filter: Option<Vec<MimeType>>,
        opts: SetOptions,
    ) -> Result<SetPlan> {
        if self.appindex.app(app_id).is_none() {
            return Err(Error::UnknownApp(app_id.to_string()));
        }

        let mut candidates: Vec<MimeType> = Vec::new();
        let mut skipped: Vec<MimeType> = Vec::new();
        for t in &umbrella {
            if filter.as_ref().is_some_and(|f| !f.contains(t)) {
                continue;
            }
            let handled = if opts.exact {
                self.appindex.declares(app_id, t)
            } else {
                self.handles(app_id, t)
            };
            if opts.force || handled {
                candidates.push(t.clone());
            } else {
                skipped.push(t.clone());
            }
        }

        if candidates.is_empty() {
            return Err(Error::AppHandlesNothingUnderUmbrella {
                app: app_id.to_string(),
                umbrella: label,
            });
        }

        let (set_types, unchanged): (Vec<MimeType>, Vec<MimeType>) = if opts.no_clobber {
            candidates
                .into_iter()
                .partition(|t| self.defaults.current_default(t).is_none())
        } else {
            (candidates, Vec::new())
        };

        let inherited_via: Vec<InheritedSet> = set_types
            .iter()
            .filter(|t| !self.appindex.declares(app_id, t))
            .filter_map(|t| {
                self.nearest_declared_ancestor(app_id, t)
                    .map(|via| InheritedSet { mime: t.to_string(), via: via.to_string() })
            })
            .collect();

        let edits: Vec<crate::writer::Edit> = set_types
            .iter()
            .map(|t| crate::writer::Edit::Set(t.clone(), app_id.clone()))
            .collect();
        let written = if opts.dry_run || edits.is_empty() {
            false
        } else {
            crate::writer::write_user_defaults(&self.roots.user_mimeapps(), &edits)?
        };

        Ok(SetPlan {
            app: app_id.to_string(),
            target: label,
            set_types: set_types.iter().map(|t| t.to_string()).collect(),
            skipped_types: skipped.iter().map(|t| t.to_string()).collect(),
            unchanged_types: unchanged.iter().map(|t| t.to_string()).collect(),
            inherited_via,
            forced: opts.force,
            no_clobber: opts.no_clobber,
            dry_run: opts.dry_run,
            written,
        })
    }
```

This is a pure extraction — the per-type loop, guard, partition, provenance, and write logic are byte-for-byte the same as the current `set`, only the umbrella/filter/app-id now arrive as parameters.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib set 2>&1 | tail -25`
Expected: PASS — the 3 new `set_types` tests AND all existing `set_*` tests (the extraction must not regress them).

- [ ] **Step 5: Commit**

```bash
git add src/engine.rs
git commit -m "feat(engine): set_types sets defaults over an explicit mimetype list"
```

---

### Task 4: Engine `unset_many` (batch unset with per-type result)

**Files:**
- Modify: `src/engine.rs:826-832` (`unset` → thin wrapper; add `unset_many`)
- Test: `src/engine.rs` `write_tests` module

**Interfaces:**
- Consumes: `self.mimedb.canonicalize`, `self.roots.user_mimeapps()`, `crate::defaults::Defaults::load`, `crate::writer::{Edit, write_user_defaults}`.
- Produces: `pub fn unset_many(&self, mimes: &[String]) -> Result<Vec<(String, bool)>>` — per type: (canonical mime string, `removed`), where `removed` is whether the **user** file had that default. `pub fn unset(&self, mime) -> Result<bool>` becomes a wrapper returning the single `removed` value.

- [ ] **Step 1: Write the failing test**

`engine_with_temp_config("...")` copies a fixture `mimeapps.list` whose `[Default Applications]` includes `video/mp4=mpv.desktop` (see `unset_removes_existing_default`). Add to `write_tests`:

```rust
    #[test]
    fn unset_many_reports_per_type_removed() {
        let (e, path) = engine_with_temp_config("unset-many");
        let list = vec![
            "video/mp4".to_string(),       // present in fixture → removed
            "image/png".to_string(),       // not a user default → not removed
        ];
        let results = e.unset_many(&list).unwrap();
        assert_eq!(results, vec![
            ("video/mp4".to_string(), true),
            ("image/png".to_string(), false),
        ]);
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(!content.contains("video/mp4="));
    }

    #[test]
    fn unset_many_canonicalizes_keys() {
        let (e, _path) = engine_with_temp_config("unset-many-canon");
        // image/jpg is an alias; it isn't a user default, so removed == false,
        // but the reported mime is the canonical image/jpeg.
        let results = e.unset_many(&["image/jpg".to_string()]).unwrap();
        assert_eq!(results, vec![("image/jpeg".to_string(), false)]);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib unset_many 2>&1 | tail -20`
Expected: FAIL — `no method named unset_many`.

- [ ] **Step 3: Implement `unset_many` and rewrite `unset`**

In `src/engine.rs`, replace the existing `pub fn unset`:

```rust
    /// Remove the user default for `mime`. Returns whether the user file changed.
    pub fn unset(&self, mime: &str) -> Result<bool> {
        Ok(self.unset_many(std::slice::from_ref(&mime.to_string()))?[0].1)
    }

    /// Remove user defaults for each of `mimes` (alias-canonicalized), in one
    /// batched write. Returns `(canonical_mime, removed)` per input, where
    /// `removed` is whether the USER mimeapps.list had that default (i.e. the
    /// write actually dropped it). Deletes are batched; a type absent from the
    /// user file is reported `removed: false` and changes nothing.
    pub fn unset_many(&self, mimes: &[String]) -> Result<Vec<(String, bool)>> {
        let user = self.roots.user_mimeapps();
        // User-file-only view (NOT the merged precedence chain) so `removed`
        // reflects what this write can actually drop.
        let user_defaults = Defaults::load(&[user.clone()])?;
        let canon: Vec<MimeType> = mimes
            .iter()
            .map(|m| self.mimedb.canonicalize(&MimeType::new(m.as_str())))
            .collect();
        let results: Vec<(String, bool)> = canon
            .iter()
            .map(|t| (t.to_string(), user_defaults.current_default(t).is_some()))
            .collect();
        let edits: Vec<crate::writer::Edit> =
            canon.into_iter().map(crate::writer::Edit::Unset).collect();
        crate::writer::write_user_defaults(&user, &edits)?;
        Ok(results)
    }
```

> `Defaults` is already imported at the top of `engine.rs` (`use crate::defaults::Defaults;`).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib unset 2>&1 | tail -25`
Expected: PASS — both new tests AND the existing `unset_removes_existing_default` (the wrapper preserves its bool contract).

- [ ] **Step 5: Commit**

```bash
git add src/engine.rs
git commit -m "feat(engine): unset_many batch-removes defaults with per-type result"
```

---

### Task 5: Thread an injectable stdin into the CLI core (pure refactor)

**Files:**
- Modify: `src/cli.rs` — `run_command`, `execute`, `run`; add `execute_with_stdin`, `stdin_is_source`, `read_type_lines`.

**Interfaces:**
- Produces:
  - `pub fn execute_with_stdin(engine: &Engine, command: &Command, json: bool, show_all: bool, stdin: &mut dyn std::io::Read, stdin_is_tty: bool) -> Outcome` — the new testable core.
  - `pub fn execute(engine, command, json, show_all) -> Outcome` — unchanged signature; wrapper that calls `execute_with_stdin` with `&mut std::io::empty()` and `stdin_is_tty = true` (so no command reads stdin). Keeps every existing call site/test compiling untouched.

This task is a behavior-preserving refactor: it threads the reader + TTY flag through to `run_command`, but the `Set`/`Unset` arms still call the existing engine methods exactly as before. The two stdin helpers (`stdin_is_source`, `read_type_lines`) are added in Task 6 at their first use, to avoid a dead-code window here. The gate is "all existing tests still pass."

- [ ] **Step 1: Add the imports**

At the top of `src/cli.rs`, add the `Read`/`IsTerminal` imports next to the existing `use` lines (both are used in this task: `Read` in the new signature, `IsTerminal` in `run`):

```rust
use std::io::{IsTerminal, Read};
```

- [ ] **Step 2: Thread stdin through `run_command` and `execute`**

Change `fn run_command` signature to accept the reader + tty flag:

```rust
fn run_command(
    engine: &Engine,
    command: &Command,
    json: bool,
    show_all: bool,
    stdin: &mut dyn Read,
    stdin_is_tty: bool,
) -> Result<String, Error> {
```

Replace the current `pub fn execute` with the new core plus a back-compat wrapper:

```rust
/// Run a command against the engine, with an injectable stdin (reader + TTY
/// status), and capture rendered output + exit code.
pub fn execute_with_stdin(
    engine: &Engine,
    command: &Command,
    json: bool,
    show_all: bool,
    stdin: &mut dyn Read,
    stdin_is_tty: bool,
) -> Outcome {
    match run_command(engine, command, json, show_all, stdin, stdin_is_tty) {
        Ok(stdout) => Outcome { code: 0, stdout, stderr: String::new() },
        Err(e) => render_error(&e, json),
    }
}

/// Back-compat entry: no stdin available (empty reader, treated as a TTY), so no
/// command reads from stdin. Used by tests and any non-piped caller.
pub fn execute(engine: &Engine, command: &Command, json: bool, show_all: bool) -> Outcome {
    execute_with_stdin(engine, command, json, show_all, &mut std::io::empty(), true)
}
```

Inside `run_command`, the `Set` and `Unset` arms do **not** change yet. The two new params are unused this task, which would warn; silence it by adding, as the very first line of `run_command`'s body, a discard that consumes both (removed in Task 6 when the arms use them):

```rust
    let _ = (&mut *stdin, stdin_is_tty);
```

- [ ] **Step 3: Wire real stdin in `run`**

In `pub fn run`, build the engine arm to pass real stdin:

```rust
pub fn run() -> i32 {
    let cli = Cli::parse();
    let roots = Roots::from_env();
    let stdin = std::io::stdin();
    let is_tty = stdin.is_terminal();
    let outcome = match &cli.command {
        Command::Init { force } => {
            init_outcome(&roots.data_home.join("madft/categories.toml"), *force, cli.json)
        }
        cmd => match Engine::load(&roots, &current_desktops()) {
            Ok(engine) => {
                let mut lock = stdin.lock();
                execute_with_stdin(&engine, cmd, cli.json, cli.all, &mut lock, is_tty)
            }
            Err(e) => render_error(&e, cli.json),
        },
    };
    if !outcome.stdout.is_empty() {
        println!("{}", outcome.stdout);
    }
    if !outcome.stderr.is_empty() {
        eprintln!("{}", outcome.stderr);
    }
    outcome.code
}
```

- [ ] **Step 4: Run the full suite to verify no regression**

Run: `cargo test 2>&1 | tail -15`
Expected: PASS — the entire existing suite is green (the `execute` wrapper keeps every test call site valid). A `dead_code` warning for `parse_type_lines` (test-only-used until Task 6) is expected at this point and resolves in Task 6; there must be **no `unused_variables` warning** for `stdin`/`stdin_is_tty` (the discard line silences them).

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs
git commit -m "refactor(cli): injectable stdin reader + tty flag in execute core"
```

---

### Task 6: `set` reads its type list from stdin

**Files:**
- Modify: `src/cli.rs` `Command::Set` arm in `run_command`
- Test: `src/cli.rs` `tests` module

**Interfaces:**
- Consumes: `engine.set_types` (Task 3), `stdin_is_source`, `read_type_lines` (Task 5), `Error::{ConflictingTypeSource, EmptyTypeList}` (Task 1).

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/cli.rs` (note: these call `execute_with_stdin` directly with a byte slice and `stdin_is_tty = false`):

```rust
    #[test]
    fn set_dash_target_reads_types_from_stdin() {
        let cmd = Command::Set {
            app: "mpv".to_string(),
            target: Some("-".to_string()),
            types: vec![],
            force: false,
            no_clobber: false,
            exact: false,
            dry_run: true,
        };
        let mut input = b"video/mp4\naudio/mpeg\n".as_slice();
        let out = execute_with_stdin(&engine(), &cmd, true, false, &mut input, false);
        assert_eq!(out.code, 0);
        let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(v["target"], "(stdin)");
        assert_eq!(v["set_types"], serde_json::json!(["video/mp4", "audio/mpeg"]));
    }

    #[test]
    fn set_implicit_piped_stdin_when_no_target() {
        let cmd = Command::Set {
            app: "mpv".to_string(),
            target: None,
            types: vec![],
            force: false,
            no_clobber: false,
            exact: false,
            dry_run: true,
        };
        let mut input = b"video/mp4\n".as_slice();
        // stdin_is_tty = false => implicit trigger.
        let out = execute_with_stdin(&engine(), &cmd, true, false, &mut input, false);
        let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(v["set_types"], serde_json::json!(["video/mp4"]));
    }

    #[test]
    fn set_real_target_ignores_stdin() {
        let cmd = Command::Set {
            app: "mpv".to_string(),
            target: Some("Media".to_string()),
            types: vec![],
            force: false,
            no_clobber: false,
            exact: false,
            dry_run: true,
        };
        // Even with piped stdin, a real target wins and stdin is untouched.
        let mut input = b"video/mp4\n".as_slice();
        let out = execute_with_stdin(&engine(), &cmd, true, false, &mut input, false);
        let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(v["target"], "Media");
    }

    #[test]
    fn set_empty_stdin_errors() {
        let cmd = Command::Set {
            app: "mpv".to_string(),
            target: Some("-".to_string()),
            types: vec![],
            force: false,
            no_clobber: false,
            exact: false,
            dry_run: true,
        };
        let mut input = b"\n   \n".as_slice();
        let out = execute_with_stdin(&engine(), &cmd, true, false, &mut input, false);
        assert_eq!(out.code, 1);
        let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(v["error"]["kind"], "empty-type-list");
    }

    #[test]
    fn set_types_flag_plus_stdin_conflicts() {
        let cmd = Command::Set {
            app: "mpv".to_string(),
            target: Some("-".to_string()),
            types: vec!["video/mp4".to_string()],
            force: false,
            no_clobber: false,
            exact: false,
            dry_run: true,
        };
        let mut input = b"audio/mpeg\n".as_slice();
        let out = execute_with_stdin(&engine(), &cmd, true, false, &mut input, false);
        assert_eq!(out.code, 1);
        let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(v["error"]["kind"], "conflicting-type-source");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib cli::tests::set_ 2>&1 | tail -25`
Expected: FAIL — current arm ignores stdin: `target` is not `(stdin)`, conflict/empty errors not produced.

- [ ] **Step 3: Add the stdin helpers, then rewrite the `Set` arm**

First delete the temporary `let _ = (&mut *stdin, stdin_is_tty);` line added in Task 5 (the names are genuinely used now). Then add the two helpers after `fn parse_type_lines` in `src/cli.rs` (first used here, also used by Task 7's `Unset` arm):

```rust
/// Whether the operand list should be read from stdin: an explicit `-` target,
/// or no positional target on a non-TTY (piped) stdin.
fn stdin_is_source(positional: Option<&str>, is_tty: bool) -> bool {
    matches!(positional, Some("-")) || (positional.is_none() && !is_tty)
}

/// Read all of stdin and parse it as a newline-delimited mimetype list. An I/O
/// error converts to `Error::Io` via `?`.
fn read_type_lines(stdin: &mut dyn Read) -> Result<Vec<String>, Error> {
    let mut buf = String::new();
    stdin.read_to_string(&mut buf)?;
    Ok(parse_type_lines(&buf))
}
```

(Match `cli.rs`'s existing style — `run_command` already returns the two-arg `Result<_, Error>` rather than the `crate::error::Result` alias.)

Now in `run_command`, replace the `Command::Set { .. }` arm.

```rust
        Command::Set { app, target, types, force, no_clobber, exact, dry_run } => {
            let opts = SetOptions { force: *force, no_clobber: *no_clobber, exact: *exact, show_all, dry_run: *dry_run };
            if stdin_is_source(target.as_deref(), stdin_is_tty) {
                if !types.is_empty() {
                    return Err(Error::ConflictingTypeSource);
                }
                let list = read_type_lines(stdin)?;
                if list.is_empty() {
                    return Err(Error::EmptyTypeList);
                }
                let r = engine.set_types(app, &list, opts)?;
                if json { to_json(&r) } else { human_set(&r) }
            } else {
                let filter = if types.is_empty() { None } else { Some(types.as_slice()) };
                let r = engine.set(app, target.as_deref(), filter, opts)?;
                if json { to_json(&r) } else { human_set(&r) }
            }
        }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib cli::tests::set_ 2>&1 | tail -25`
Expected: PASS (5 new tests). Then `cargo test 2>&1 | tail -8` — whole suite green.

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): set reads its type list from stdin (- or piped)"
```

---

### Task 7: `unset` reads its mimetype list from stdin

**Files:**
- Modify: `src/cli.rs:66-67` (`Command::Unset` field → `Option<String>`), `Command::Unset` arm in `run_command`
- Test: `src/cli.rs` `tests` module

**Interfaces:**
- Consumes: `engine.unset_many` (Task 4), `engine.unset` (single path), `stdin_is_source`, `read_type_lines`, `Error::{EmptyTypeList, MissingMimetype}`.
- Note: `Command::Unset` becomes `Unset { mimetype: Option<String> }`. Every match/construction of `Unset` updates accordingly.

- [ ] **Step 1: Write the failing tests**

`unset` WRITES (no dry-run), so the list tests must NOT use the shared read-only
`engine()` (it points at committed fixtures under `tests/fixtures/engine/config`
— a write there would mutate fixtures and drop a `.bak`). Add a temp-config
helper (mirrors the existing `human_app_marks_undeclared_default_row` pattern)
and use it. Add to the `tests` module in `src/cli.rs`:

```rust
    /// An engine whose config_home is a fresh temp dir seeded with one default,
    /// so `unset` can write without touching committed fixtures.
    fn engine_writable(tag: &str) -> Engine {
        let f = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let cfg = std::env::temp_dir().join(format!("madft-cli-{tag}"));
        let _ = std::fs::remove_dir_all(&cfg);
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(
            cfg.join("mimeapps.list"),
            "[Default Applications]\nvideo/mp4=mpv.desktop\n",
        )
        .unwrap();
        let roots = Roots {
            data_home: f.join("engine"),
            data_dirs: vec![f.clone()],
            config_home: cfg,
            config_dirs: vec![],
        };
        Engine::load(&roots, &[]).unwrap()
    }

    #[test]
    fn unset_stdin_list_json_per_type() {
        // temp config has video/mp4=mpv.desktop; image/png has no user default.
        let e = engine_writable("unset-stdin-json");
        let cmd = Command::Unset { mimetype: Some("-".to_string()) };
        let mut input = b"video/mp4\nimage/png\n".as_slice();
        let out = execute_with_stdin(&e, &cmd, true, false, &mut input, false);
        assert_eq!(out.code, 0);
        let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(v["unset"][0]["mime"], "video/mp4");
        assert_eq!(v["unset"][0]["removed"], true);
        assert_eq!(v["unset"][1]["mime"], "image/png");
        assert_eq!(v["unset"][1]["removed"], false);
        assert_eq!(v["removed_count"], 1);
    }

    #[test]
    fn unset_stdin_list_human_per_line() {
        let e = engine_writable("unset-stdin-human");
        let cmd = Command::Unset { mimetype: None };
        let mut input = b"video/mp4\nimage/png\n".as_slice();
        let out = execute_with_stdin(&e, &cmd, false, false, &mut input, false);
        assert_eq!(out.code, 0);
        assert_eq!(out.stdout, "unset video/mp4\nimage/png: no user default to remove");
    }

    #[test]
    fn unset_single_mimetype_unchanged_json() {
        // Single real arg keeps today's {unset, written} shape byte-for-byte.
        // image/png has no default in the temp config => no write, written:false.
        let e = engine_writable("unset-single");
        let cmd = Command::Unset { mimetype: Some("image/png".to_string()) };
        let out = execute(&e, &cmd, true, false);
        let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(v["unset"], "image/png");
        assert_eq!(v["written"], false);
    }

    #[test]
    fn unset_no_arg_on_tty_errors() {
        let cmd = Command::Unset { mimetype: None };
        // is_tty = true, no positional => missing-mimetype.
        let out = execute_with_stdin(&engine(), &cmd, true, false, &mut std::io::empty(), true);
        assert_eq!(out.code, 1);
        let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(v["error"]["kind"], "missing-mimetype");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib cli::tests::unset_ 2>&1 | tail -25`
Expected: FAIL — `Unset` has no `mimetype: Option` yet / arm doesn't branch on stdin (compile error first).

- [ ] **Step 3: Make the field optional + rewrite the arm**

In `src/cli.rs`, change the `Unset` variant:

```rust
    /// Remove the user default for a mimetype, or for each mimetype on stdin.
    Unset { mimetype: Option<String> },
```

Replace the `Command::Unset { mimetype }` arm in `run_command`:

```rust
        Command::Unset { mimetype } => {
            if stdin_is_source(mimetype.as_deref(), stdin_is_tty) {
                let list = read_type_lines(stdin)?;
                if list.is_empty() {
                    return Err(Error::EmptyTypeList);
                }
                let results = engine.unset_many(&list)?;
                if json {
                    let items: Vec<_> = results
                        .iter()
                        .map(|(mime, removed)| serde_json::json!({ "mime": mime, "removed": removed }))
                        .collect();
                    let removed_count = results.iter().filter(|(_, r)| *r).count();
                    to_json(&serde_json::json!({ "unset": items, "removed_count": removed_count }))
                } else {
                    results
                        .iter()
                        .map(|(mime, removed)| {
                            if *removed {
                                format!("unset {mime}")
                            } else {
                                format!("{mime}: no user default to remove")
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            } else {
                match mimetype {
                    None => return Err(Error::MissingMimetype),
                    Some(mimetype) => {
                        let wrote = engine.unset(mimetype)?;
                        if json {
                            to_json(&serde_json::json!({ "unset": mimetype, "written": wrote }))
                        } else if wrote {
                            format!("unset {mimetype}")
                        } else {
                            format!("{mimetype}: no user default to remove")
                        }
                    }
                }
            }
        }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib cli::tests::unset_ 2>&1 | tail -25`
Expected: PASS (4 new tests). Then `cargo test 2>&1 | tail -8` — whole suite green.

- [ ] **Step 5: Verify `-` parses as a positional (manual smoke)**

Run: `echo "video/mp4" | cargo run -q -- unset - --json 2>&1 | tail -5`
Expected: JSON with an `unset` array (NOT a clap "unexpected argument" error). If clap rejects the bare `-`, add `#[arg(allow_hyphen_values = true)]` to the `Set { target }` and `Unset { mimetype }` fields and re-run.

- [ ] **Step 6: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): unset reads its mimetype list from stdin (- or piped)"
```

---

### Task 8: Documentation

**Files:**
- Modify: `README.md:43-48` (everyday-use examples), `README.md:63-64` (command table)

**Interfaces:** none (docs only).

- [ ] **Step 1: Update the everyday-use block**

In `README.md`, after the `madft unset video/mp4` line (around line 47), add:

```bash
# Pipe a computed list of types (newline-delimited) into set/unset:
madft app firefox --json | jq -r '.types[] | select(.category==null) | .mime' \
  | madft set firefox - --no-clobber   # set firefox for its uncategorized types
madft types Media.Video | madft unset -   # clear defaults for every video type
```

- [ ] **Step 2: Update the command table rows**

Replace the `set` and `unset` rows (lines 63-64):

```markdown
| `set <app> [PATH\|mimetype\|-] [--types a,b] [-f/--force] [--no-clobber] [--exact] [--dry-run]` | Set `app` as default for the umbrella's types it handles — declared **or** reachable via a parent type. A `-` target (or piped stdin with no target) reads a **newline-delimited mimetype list from stdin**, which becomes the operand set directly (bypasses the category tree — good for uncategorized types); incompatible with `--types`. `--exact` restricts to literally-declared types; `--force` overrides entirely; `--no-clobber` fills only unset; `--dry-run` previews. |
| `unset [mimetype\|-]` | Remove the user default for a type. A `-` argument (or piped stdin with no argument) removes the default for **each mimetype on stdin** (newline-delimited), reporting per type. |
```

- [ ] **Step 3: Verify the docs build cleanly (no test, just a render check)**

Run: `grep -n "madft set firefox -" README.md`
Expected: the new example line is present.

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "docs: document stdin type-list piping for set and unset"
```

---

## Self-Review

**Spec coverage:**
- §1 Trigger (`-` sentinel + implicit non-TTY, real target wins, unset optional + tty-no-arg error) → `stdin_is_source` (T5), Set arm (T6), Unset arm + `MissingMimetype` (T1/T7). ✓
- §2 Line protocol (`parse_type_lines`) → T2. ✓
- §3 `set` semantics (raw umbrella, canonical, guards reused, `(stdin)` label) → `set_types`/`set_core` (T3), Set arm (T6). ✓
- §4 `unset` semantics (batch, canonical, per-type removed, human/JSON shapes, single-arg byte-identical) → `unset_many` (T4), Unset arm (T7). ✓
- §5 Guard rails (`--types`+stdin conflict, empty-stdin error) → `ConflictingTypeSource`/`EmptyTypeList` (T1), enforced in arms (T6/T7). ✓
- §6 Structure/testing (injectable reader, TTY only for implicit, shared helpers, pure parser) → T5 + tests throughout. ✓

**Placeholder scan:** No TBD/TODO; every code step shows complete code; every test shows assertions. ✓

**Type consistency:** `set_types(&self, app: &str, types: &[String], opts: SetOptions) -> Result<SetPlan>`, `unset_many(&self, mimes: &[String]) -> Result<Vec<(String, bool)>>`, `stdin_is_source(Option<&str>, bool) -> bool`, `read_type_lines(&mut dyn Read) -> Result<Vec<String>>`, `execute_with_stdin(..., &mut dyn Read, bool) -> Outcome` — names/signatures used consistently across T3–T7. Error kinds `conflicting-type-source` / `empty-type-list` / `missing-mimetype` match between T1 and the tests in T6/T7. ✓
