# CLAUDE.md

Instructions for Claude when working on this project.

## Communication Mode

**Default: `/caveman ultra`.** Drop articles, filler, hedging. Abbreviate (DB/auth/config/req/res/fn/impl). Arrows for causality (X → Y). Fragments OK.

Exceptions — drop caveman temporarily: security warnings, destructive-op confirms, multi-step sequences where fragment order risks misread, user asks to clarify.

**Never cavemanize:** code, commit messages, PR descriptions, TOML fixtures, error strings.

**Off switch:** "stop caveman" / "normal mode". Subagents get explicit `/caveman ultra` in prompt.

## Recording key decisions safely

Sessions get summarized + compacted. Long chains of debugging that don't write down their _findings_ lose them. Record key decisions IMMEDIATELY when made — don't wait for end-of-task or commit time, those moments may never arrive.

**Three persistence layers, distinct purposes — never duplicate:**

| Layer                        | What goes there                                                                         | Update trigger                                                  |
| ---------------------------- | --------------------------------------------------------------------------------------- | --------------------------------------------------------------- |
| **CLAUDE.md** (this file)    | Project conventions, current focus, deferred edges, algorithmic insights, cross-cop config rules | Right after a non-obvious decision lands or unblocks            |
| **ARCHITECTURE.md**          | Runtime / registration / autocorrect / testing pipeline shape                           | When that pipeline structure changes (rare)                     |
| **MEMORY.md** + memory files | User profile, recurring feedback, project-state pointers, external-system references    | When user states a preference, correction, or external resource |
| **COPS.md**                  | Cop coverage matrix + autocorrect counts (single source of truth for progress)          | Every autocorrect-wiring commit (line 8 + table + Total)        |

**What counts as a "key decision" worth recording (ordered by priority):**

1. **Algorithmic mismatch resolved** — when our cop's algorithm diverged from RuboCop's and the fix is non-obvious (e.g. RuboCop uses _two_ different start-nodes for offense vs autocorrect; HashAlignment registers offenses under winning style but corrects with first-configured style). Record the divergence + why RuboCop does it that way, not just the patch. → "Current focus" Status line + Known deferred.
2. **Cross-cop coupling discovered** — a cop reading another's config, a fixture's pre-baked indentation, a TOML extraction quirk. → bottom of Conventions section.
3. **Prism vs RuboCop AST shape difference** — block-as-child-of-call (Prism) vs block-as-parent-of-send (RuboCop), Option vs non-Option locs, etc. → "Prism API gotchas" subsection.
4. **Tester / decode_source / applier behavior** — the hidden contract between fixtures and the cop runtime. → "Testing" or "Conventions" section.
5. **Why a deferral is or is no longer real** — when a "deferred edge case" turns out to be tractable by porting more RuboCop logic, mark closed and explain how. → "Known deferred edge cases".

**Anti-patterns** (do NOT do):

- ❌ Marking a fixture `pending = true` to make tests pass when the issue is in our cop logic. Fix the cop, do not silence the test. (Genuine RuboCop-pending fixtures are the only legitimate `pending = true`.)
- ❌ Recording the patch without the _insight_. "Fixed BlockAlignment" → useless. "BlockAlignment chain alignment requires a _second_ walk past the find_align_frame stopping point — RuboCop calls this start_for_line_node — that picks the topmost ancestor still on the same line; offense detection uses the first walk's column, autocorrect target uses the second" → reconstructable.
- ❌ Burying decisions in commit messages only. Git log lookup is fine for _what_ changed; CLAUDE.md is for _why_ and _what to remember next time_. Both are needed; neither substitutes for the other.
- ❌ Letting MEMORY.md and CLAUDE.md drift into duplication. CLAUDE.md = project-scoped, in-repo, applies to anyone. MEMORY.md = user-scoped, persistent across sessions, applies to _this user's_ preferences and references.

**Format discipline:**

- New "Current focus" status entries: one line, dated implicitly by position, `+N corrections` if quantifiable.
- Known deferred edges: one bullet per residual with a one-clause "why" (algorithm gap / multi-pass / fixture quirk).
- Numbers must come from `cargo test --test tester 2>&1 | grep "Corrections validated"`, not memory.

## Project Overview

ruby-fast-cop = Rust port of RuboCop. Target 50-100x faster (like Ruff:Python).

**State:** 606/606 cops (396/396 enabled-by-default; 156/156 pending-by-default; 54/54 disabled-by-default). ~28,053 test cases from RuboCop v1.85.0 RSpec, all green.

> **Architecture:** see [`ARCHITECTURE.md`](./ARCHITECTURE.md) for runtime shape, registration, autocorrect pipeline, testing pipeline. CLAUDE.md = conventions; ARCHITECTURE.md = structure. Update ARCHITECTURE.md only when runtime/registration/autocorrect/testing shape changes.

## Current focus: autocorrect coverage

All 606 cops implemented. **Active workstream = wiring `Correction` emission** so `cargo test --test tester` passes the strict-mode `corrected` block check for every fixture that has one.

**Status:** 11,219 / 11,219 (100%) corrections wired. **Phase 0 #3 closed.** Last residuals fixed by porting RuboCop logic instead of marking pending: BlockAlignment chain via `start_for_line_node` topmost-same-line walk; HashAlignment prefer-table mixed-style via per-style column_deltas + first-configured-style correction; LineLength YARD-tab via `decode_source` always-prepend-base_indent. Only 3 RuboCop-pending fixtures remain skipped (loop-body liveness, mixed tab/space). Per-dept totals in `COPS.md` summary.

Tester is hard-flipped: any TOML `corrected` block with no matching `Correction` from the cop = test failure. No silent skips. See `tests/tester.rs` ~L420 for the gate.

**Phase 0 close — algorithmic insights to keep:**

- **Layout/BlockAlignment chain alignment** (`src/cops/layout/block_alignment.rs`) — RuboCop uses two distinct ancestor walks: `start_for_block_node` (offense detection / message) and `start_for_line_node` (autocorrect target — the topmost ancestor still on the same line as the find_align_frame result). For `bar.get_stuffs.reject{}.select{}` chains the first walk stops at `.select_outer` (col 6) but the second walk lifts to `bar` at col 2. Added `find_topmost_same_line_lhs_start()`; Either + StartOfLine autocorrect targets use it. Splat tests work via the same walk crossing `Hash[]`/splat into the assignment frame on same line. Offense detection still uses the original (un-walked) frame to preserve RuboCop's `start_loc.column != end_loc.column` check.
- **Layout/HashAlignment prefer-table mixed-style** (`src/cops/layout/hash_alignment.rs`) — RuboCop's `register_offenses_with_format` registers offenses under the _winning_ (least-offenses) style's MESSAGE but corrects using `column_deltas[alignment_for(offense).first.class]` — the _first configured_ style's delta. So `EnforcedHashRocketStyle = ["key", "table"]` with table winning reports under table-message but applies key-style correction. Refactored `check_pairs_alignment` to track `(style, pair_idx) → delta` and `style → offending_indexes` separately, then build offenses post-hoc using first-style's delta.
- **decode_source always-prepend-base_indent** (`tests/tester.rs`) — previously a tab-led-line special case left base_indent off some lines, breaking column math for fixtures hitting Layout/LineLength's YARD-style tab path. Made the prepending unconditional for non-blank lines.

**Known deferred edge cases**: NONE blocking. The 3 `pending = true` fixtures are RuboCop-pending in upstream RSpec metadata, not local deferrals: Layout/IndentationWidth×2 (mixed tab+space) and Lint/UselessAssignment (loop-body liveness — RuboCop's own VariableForce can't analyze yet).

## Production-readiness roadmap

Current state: **alpha-internal**. 606/606 cops implemented, 11,219/11,219 (100%) autocorrect wired, 28k synthetic tests green.

### Phase 0 — autocorrect 100% (DONE)

Closed. `cargo test --test tester` reports `Corrections validated: 11219` with zero failures. Multi-pass applier (`check_and_correct_source_full`, `src/lib.rs:163`) iterates to fixed-point with hash-cycle detection. Three pending fixtures remain skipped — all RuboCop-pending in upstream metadata (Layout/IndentationWidth mixed tab+space ×2, Lint/UselessAssignment loop-body liveness).

### Phase 1 — public alpha (1-2w, after Phase 0 closes)

Goal: outsider can run on their codebase without embarrassing divergence from RuboCop.

1. **Real-world corpus parity** — pick 3 OSS Ruby repos (Rails, Discourse, Shopify, Mastodon, fastlane). Run both `rubocop` and `ruby-fast-cop`; diff offense counts per cop. Target ±1% parity. Will surface parser edge cases, config inheritance gaps, encoding/line-ending bugs.
2. **Config edges** — `inherit_from`, `inherit_gem`, glob `Include`/`Exclude`, brace-expand. Likely blockers for #1.

Exit: parity diff ≤1% on 3 corpora, no parser crashes.

### Phase 2 — beta (2-3w)

Goal: editor-integratable, scriptable, faster than RuboCop on benchmarks.

4. **CLI completeness** — `--only`/`--except`/`--require`, formatters (`-f json`/`-f emacs`/`-f github`/`-f junit`), `--parallel`, `--cache`, `--auto-gen-config`, `-a`/`-A` wired through library API.
5. **LSP E2E** — exercise via VS Code Ruby LSP / Zed / Neovim. Validate diagnostics, code-actions, formatting-on-save. Likely uncovers incremental-parse bugs.
6. **Benchmarks** — repro suite vs RuboCop on Phase-1 corpora. Publish: parse-only, lint-only, lint+autocorrect, cold vs warm, single-file vs whole-repo. Target 50-100x. Decide ship-vs-chase if actual is 20x.
7. **Dogfooding** — `.rubocop.yml` for project's own scripts (`.claude/skills/*/scripts/*.rb`); wire CI gate.

Exit: editor diagnostics work, benchmarks published, CI gating on self-lint.

### Phase 3 — 1.0 release (1-2w)

Goal: anyone can `brew install` / `cargo install` and adopt.

8. **API stability** — finalize public lib surface (`check_source`, `check_and_correct`, `Config`, `Offense`, `Correction`). Mark internals `pub(crate)`. `cargo doc`.
9. **Release artifacts** — `cargo publish` (crate + bin), Homebrew formula, prebuilt binaries (mac arm64/x86, linux x86/arm, windows) via GHA, npm wrapper (Ruff pattern) for editor adoption.
10. **Docs site** — README quickstart, RuboCop migration guide (config compat), per-cop docs (auto-gen from TOML), CHANGELOG.
11. **1.0 tag** — semver lock, deprecation policy, security-disclosure path.

### Out-of-band

- Architectural refactors (next section) — opportunistic when touching adjacent code.
- Pending/Disabled cop autocorrect — implemented but partial wiring; lower priority.
- Plugin loading (rubocop-rails/rspec/performance) — post-1.0 ecosystem play.

### Critical-path estimate

| Phase                | Time     | Risk                                      |
| -------------------- | -------- | ----------------------------------------- |
| 0 — autocorrect 100% | 3-5d     | multi-pass termination edge cases         |
| 1 — public alpha     | 1-2w     | corpus parity may surface deep bugs       |
| 2 — beta             | 2-3w     | benchmark numbers determine perf strategy |
| 3 — 1.0              | 1-2w     | mostly packaging, low risk                |
| **Total**            | **5-8w** |                                           |

Biggest unknown: corpus diff. If >5% per-cop, debt blows up. Mitigate by diffing per-cop and tackling worst offenders first.

### Recommended next concrete actions (in order)

1. **Phase 1 #1**: pick first OSS corpus (Rails or Discourse). Run both `rubocop` and `ruby-fast-cop`; diff offense counts per cop; tackle worst-divergence cops first.
2. **Phase 1 #2**: implement `inherit_from` / `inherit_gem` config inheritance + glob `Include`/`Exclude` (likely blockers surfaced by #1).
3. Update parity report after each corpus; aim ±1% per-cop before moving to Phase 2.

## Planned architectural refactors

Candidates to trim verbosity. Revisit when touching adjacent code.

1. **Typed config helper `Config::typed::<T>(cop_name)`** — replaces `.get_cop_config(...).raw.get(...)` chains across 184 cops via serde structs. ~1000 LOC saved.
2. **`Emitter` instead of `Vec<Offense>`** — zero-alloc on empty-offense hot path.
3. **`#[cop("Name")]` attr macro** — collapses `register_cop!` closure. Pairs with #1.
4. **Shared semantic model (scopes / CFG / comment index)** — compute once per file; today VariableForce rebuilds per-cop. High payoff, high risk.
5. **Collapse `Cop` trait 20 methods → 1 `check(&Node, &mut Emitter, &Ctx)`** — mechanical, big trait-surface win.
6. **Autocorrect conflict resolver** — Ruff-style interval tree vs "skip overlaps"; more fixes per pass.
7. **More `CheckContext` helpers** — port RuboCop `RangeHelp`/`Alignment` as need arises.

## Conventions

### Boilerplate

- `node_name!(node)` macro (src/lib.rs) instead of `String::from_utf8_lossy(node.name().as_slice())`. Works on any Prism node with `.name().as_slice()`.
- **No inline unit tests** in cop files. All testing via TOML fixtures. No `#[cfg(test)] mod tests`.
- **`#[derive(Default)]`** when `new()` returns `Self` / all fields zero-default. Manual `impl Default` only when defaults differ.
- **Register via `register_cop!`** at bottom of cop file. Self-contained — no edits to `lib.rs`, `cops/mod.rs`, or dept `mod.rs` (beyond the `mod` + `pub use`).

### Cop registration (auto via `inventory`)

Each cop file ends with one `register_cop!`. No central list. No match arms.

```rust
// No-config
crate::register_cop!("Lint/Debugger", |_cfg| Some(Box::new(Debugger::new())));

// With YAML config
crate::register_cop!("Lint/AssignmentInCondition", |cfg| {
    let allow = cfg.get_cop_config("Lint/AssignmentInCondition")
        .and_then(|c| c.allow_safe_assignment).unwrap_or(true);
    Some(Box::new(AssignmentInCondition::new(allow)))
});
```

`src/cops/registry.rs` provides `build_from_config` / `build_one` / `all_with_defaults`. Adding a cop never requires editing these.

### Offense range gotchas (`Location::from_offsets`)

Fixtures capture RuboCop's `expect_offense` `^` markers — **always ≥ 1 column wide** even for zero-width ranges. Two widening rules match this:

1. **Zero-width** (`start == end`) → `last_col = start_col + 1`. Emit zero-width when translating RuboCop's zero-width `add_offense` (e.g. `side_space_range` over a newline); widening is free.
2. **Range starting at newline byte** → newline = 1 display col, so `last_col = col_at_newline + 1`.

Do **not** broaden to "any multi-line range" — regressed 30+ tests (LineLength, FirstHashElementIndentation, Next, SymbolProc).

### Cross-cop config → gate on `is_cop_enabled`

When cop A reads cop B's config (e.g. GuardClause reads Layout/LineLength.Max), **gate on `config.is_cop_enabled("Layout/LineLength")` first**. Fixtures often set `Enabled = false` but leave `Max = 80` → false positives otherwise.

```rust
let max_line = if config.is_cop_enabled("Layout/LineLength") {
    config.get_cop_config("Layout/LineLength").and_then(|c| c.max).map(|m| m as usize)
} else { None };
```

### Prism API gotchas (sync with `.claude/skills/ruby-prism-api`)

- `Node`, `IfNode`, `UnlessNode` do **not** `Clone`/`Copy`. Helpers take `&IfNode<'a>`. No `node.clone()`.
- `Vec<Node>::clone()` fails for same reason. Move the Vec in, re-iterate parent's `StatementsNode` for a shared walk.
- No `ruby_prism::visit_node` dispatcher. Inside `Visit` impl use `self.visit(node)`.
- `opening_loc()`/`closing_loc()` inconsistent: `StringNode`/`InterpolatedStringNode`/`ArrayNode`/`HashNode` → `Option<Location>`; `XStringNode`/`InterpolatedXStringNode`/`BlockNode`/`LambdaNode`/`RegularExpressionNode`/`ParenthesesNode`/`EmbeddedStatementsNode` → `Location` (no `Option`). Check `.claude/skills/ruby-prism-api/references/node-accessors.md`.
- `AssocNode::operator_loc()` → `Option<Location>`. `None` = colon (`key: val`); `Some("=>")` = rocket. Don't `.as_slice()` the Option.

## Parser & deps

- **Prism** (`ruby-prism = "1.9.0"`). Ruby 3.4 default parser, error-tolerant, parses 2.5+. Location is byte-offset only — we compute line/col.
- Other deps: `thiserror` (errors), `clap` (CLI), `serde` + `serde_yaml` (config), `toml` (fixtures), `rayon` (parallel).

## Cop / autocorrect impl strategy

- **Translate from RuboCop source**, don't reinvent. Battle-tested edge cases — applies to autocorrect logic too: read RuboCop's `def autocorrect(corrector)` and translate `corrector.replace`/`insert_before`/`remove` calls to `Edit { start_offset, end_offset, replacement }`.
- Fetch: `https://raw.githubusercontent.com/rubocop/rubocop/master/lib/rubocop/cop/{dept}/{name}.rb` + mixins.
- 100-line Ruby cop → ~150-250 LOC Rust. Not 500+. Match RuboCop structure.
- Shared mixin (e.g. VariableForce) → mirror file structure in `src/helpers/{mixin}/`. No monoliths.
- **Never hardcode fixes to pass specific tests.** Understand RuboCop behavior first, implement generally. If a test wants a specific output, make the _general algorithm_ produce it — don't pattern-match on the test's source.

## Testing

### TOML fixture format

```toml
cop = "Style/RaiseArgs"
department = "style"
severity = "convention"
implemented = true

[[tests]]
name = "test_name"
source = '''
raise RuntimeError, 'message'
'''
corrected = '''              # optional
raise RuntimeError.new('message')
'''
base_indent = 2              # optional: restore indent before running

[[tests.offenses]]           # offenses = [] for no-offense tests
line = 1
column_start = 0
column_end = 30
message = "Provide an exception class and message as arguments to `raise`."

[tests.config]               # optional
EnforcedStyle = "exploded"
```

### Running

```bash
cargo test --test tester       # all fixtures
cargo run --bin fixture_stats  # fixture stats
```

### Extracting from RuboCop

Scripts in `.claude/skills/rubocop-test-importer/scripts/`:

- `download_rubocop_specs.sh` — clones RuboCop → `/tmp/rubocop-repo` + bundle install
- `test_data_capture.rb` — monkey-patches `RuboCop::RSpec::ExpectOffense` to capture resolved test data
- `extract_via_rspec.rb` — runs specs, generates TOML

Re-sync all:

```bash
/rubocop-test-importer sync
```

Single cop / dept:

```bash
cd /tmp/rubocop-repo && bundle exec ruby \
  /Users/naveenraj/sources/devtools/ruby-fast-cop/.claude/skills/rubocop-test-importer/scripts/extract_via_rspec.rb \
  --output /Users/naveenraj/sources/devtools/ruby-fast-cop/tests/fixtures \
  [--cop Style/RaiseArgs | --department lint]
```

### AST explorer

Prism tree dumper — confirm node types before writing match arms.

```bash
cargo run --bin ast -- 'foo.bar&.baz'             # tree + source
cargo run --bin ast -- --loc 'x.nil? ? nil : x'   # + byte offsets, 1-based line:col
cargo run --bin ast -- --no-source 'def foo; end' # structure only
cargo run --bin ast -- --file path.rb             # from file
echo 'foo || bar' | cargo run --bin ast -- --stdin
```

Output = S-expression like `(call (call (local_variable_read)))`. Translate RuboCop `def_node_matcher` patterns (`(send (send $_ :nil?) :!)`) by confirming Prism names nodes the same.

## Workflows

### Autocorrect API

```rust
use crate::offense::{Correction, Edit};

// Single edit (most common)
offense.with_correction(Correction::replace(start, end, "new text".into()))
offense.with_correction(Correction::insert(offset, "text".into()))
offense.with_correction(Correction::delete(start, end))

// Multi-edit (e.g. rewrite operator + swap operands)
let correction = Correction { edits: vec![
    Edit { start_offset: a, end_offset: b, replacement: "x".into() },
    Edit { start_offset: c, end_offset: d, replacement: "y".into() },
]};
offense.with_correction(correction)
```

Applier (`src/correction.rs`) sorts edits, walks forward, skips overlaps. No re-parse. Strict tester compares `apply_corrections(source, offenses)` against the TOML `corrected` block.

### Wiring autocorrect for one cop

1. `cargo test --test tester 2>&1 | grep -B1 -A6 "{Cop/Name}.*\(emitted no Correction\|Correction mismatch\)"` — pull every failing case.
2. Open `tests/fixtures/{dept}/{cop}.toml` — read `source` + `corrected` blocks. The diff = the rewrite to produce.
3. Cross-reference RuboCop's `def autocorrect(corrector)` in `https://raw.githubusercontent.com/rubocop/rubocop/master/lib/rubocop/cop/{dept}/{name}.rb`. Translate corrector calls → `Edit { start_offset, end_offset, replacement }`.
4. Add `with_correction(...)` at offense-creation site in the cop. Use `node.location()`, `call_operator_loc()`, `message_loc()`, `keyword_loc()` for byte offsets — most return `Location`, some return `Option<Location>` (see Prism gotchas above).
5. `cargo test --test tester` — verify zero new mismatches; the cop's "emitted no Correction" / "Correction mismatch" lines should drop to 0 (or to a documented edge-case set).
6. Update `COPS.md` summary line 8 + dept row + Total row. Numbers come from `cargo test --test tester 2>&1 | grep "Corrections validated"`.

### Reference templates for autocorrect patterns

When wiring a fix surfaced by corpus parity or a regression, look at:

- `src/cops/style/even_odd.rs` — single `Correction::replace` over offense range.
- `src/cops/style/yoda_condition.rs` — multi-edit swap with optional operator flip; `Option<Location>` handling on `message_loc()`.
- `src/cops/style/not.rs` — branching correction (flip / paren-wrap / paren-preserve / simple).
- `src/cops/style/redundant_freeze.rs` — delete trailing call.
- `src/cops/style/redundant_begin.rs` — multi-line unwrap with comment preservation.

If a fix grows past one cop and ports cleanly across a shared correction shape, run agents under `isolation="worktree", run_in_background=true, mode="bypassPermissions"`. Surgical merge: cherry-pick only the cop sources, re-derive `COPS.md` / CLAUDE.md from current `cargo test` output (agents branch from stale state). Worktree cleanup: `git worktree remove -f -f .claude/worktrees/{name}` + `git worktree prune` + `git branch -D {branch}`.

### Mandatory doc sync on every autocorrect commit

- **`COPS.md` line 8** — `Autocorrect progress: X / 11,219 (Y%)` and unwired-cop count.
- **`COPS.md` Summary table** — bump dept rows + Total row.
- Numbers source: `cargo test --test tester 2>&1 | grep "Corrections validated"` for total wired; per-dept failure delta for per-row counts. Total wired = 11,219 − (sum of correction failures across depts).
- This CLAUDE.md "Current focus" section — bump total wired; record only the algorithmic insight, not the patch list.
- ARCHITECTURE.md only if applier shape changed.

### Investigating a single failing correction

```bash
cargo test --test tester 2>&1 | grep -B1 -A8 "Failures in.*{cop}.toml"
```

Read failing test's `corrected` block in TOML → compute byte-offset diff → adjust `Edit` ranges or replacement text → re-run.

### Re-syncing fixtures (new RuboCop release)

1. Update version in `download_rubocop_specs.sh`
2. `/rubocop-test-importer sync`
3. `cargo test --test tester` — check regressions
4. Update README.md counts

## Library API

Crate = CLI binary + library. Library embedded in e.g. `ruby-fast-lsp`.

Principles: minimal stable public API; expose source-string check functions (not just file paths); core types `serde`-serializable; no AST/parser internals exposed. CLI (`main.rs`) = thin wrapper over lib.

## Performance targets

- Parse 1000 files: < 1s
- Lint 1000 files (common cops): < 2s
- 50-100x faster than RuboCop

## Environment

- Ruby (test extraction only): `/opt/homebrew/opt/ruby/bin/ruby` (Homebrew)
- RuboCop clone: `/tmp/rubocop-repo`
- RuboCop version: v1.85.0

## References

- [RuboCop docs](https://docs.rubocop.org/rubocop/)
- [Prism](https://github.com/ruby/prism) + [ruby-prism crate](https://crates.io/crates/ruby-prism)
- [Ruff](https://github.com/astral-sh/ruff) (inspiration)
