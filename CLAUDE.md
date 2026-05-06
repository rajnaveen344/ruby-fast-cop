# CLAUDE.md

Instructions for Claude when working on this project.

## Communication Mode

**Default: `/caveman ultra`.** Drop articles, filler, hedging. Abbreviate (DB/auth/config/req/res/fn/impl). Arrows for causality (X → Y). Fragments OK.

Exceptions — drop caveman temporarily: security warnings, destructive-op confirms, multi-step sequences where fragment order risks misread, user asks to clarify.

**Never cavemanize:** code, commit messages, PR descriptions, TOML fixtures, error strings.

**Off switch:** "stop caveman" / "normal mode". Subagents get explicit `/caveman ultra` in prompt.

## Project Overview

ruby-fast-cop = Rust port of RuboCop. Target 50-100x faster (like Ruff:Python).

**State:** 606/606 cops (396/396 enabled-by-default; 156/156 pending-by-default; 54/54 disabled-by-default). ~28,053 test cases from RuboCop v1.85.0 RSpec, all green.

> **Architecture:** see [`ARCHITECTURE.md`](./ARCHITECTURE.md) for runtime shape, registration, autocorrect pipeline, testing pipeline. CLAUDE.md = conventions; ARCHITECTURE.md = structure. Update ARCHITECTURE.md only when runtime/registration/autocorrect/testing shape changes.

## Current focus: autocorrect coverage

All 606 cops implemented. **Active workstream = wiring `Correction` emission** so `cargo test --test tester` passes the strict-mode `corrected` block check for every fixture that has one.

**Status:** 9,120 / 11,217 (81%) corrections wired. 2,097 expected corrections across ~131 cops still unwired. Per-cop counts in `.correction_worklist.txt`. Per-dept totals in `COPS.md` summary.

Tester is hard-flipped: any TOML `corrected` block with no matching `Correction` from the cop = test failure. No silent skips. See `tests/tester.rs` ~L420 for the gate.

Wiring proceeds **cluster-by-cluster**. A cluster = cops that share a correction shape (e.g. "redundancy removers", "swap LHS/RHS", "delete keyword"). Two clusters landed:

- **Cluster 1** (commit `6422490`) — 7 cops, +175 corrections. Yoda swap + simple replace.
- **Cluster 2** (commit `2b69077`) — 9 cops, +257 corrections. Redundancy removers (RedundantBegin, RedundantFreeze, RedundantInterpolation, RedundantReturn, RedundantSort, RedundantSortBy, Lint/RedundantSafeNavigation, Lint/RedundantSplatExpansion, Lint/SafeNavigationChain).
- **Cluster 4a** (this commit) — 2 cops, +77 corrections. Space-inside-brackets (Layout/SpaceInsideArrayLiteralBrackets, Layout/SpaceInsideReferenceBrackets). Insert/delete a single space; multi-line compact 2D-array newline cases handled by walking past whitespace.
- **Cluster 4b** (this commit) — 5 cops, +288 Layout corrections. Re-indenter cops: RescueEnsureAlignment (50→0), BlockAlignment (36→1), HeredocIndentation (60→24), FirstHashElementIndentation (31→2), HashAlignment (56→5). Also fixed heredoc_indentation.toml: 32 `corrected` blocks had base_indent pre-baked; stripped to match decode_source convention.
- **Solo: UselessAssignment** (this commit) — +49 Lint corrections. Simple-kind dead assignments deleted (`x = 1` → ``). Deferred kinds (14 residuals): MultipleAssignment, OperatorAssignment, OrAssignment, AndAssignment, RegexpNamedCapture, for-loop variable, rescued exception variable, block-local.
- **Solo: OneLineConditional** (this commit) — +54 Style corrections. Two correction modes: ternary (`if cond then a else b end` → `cond ? a : b`) and multiline (preserve keyword form when `AlwaysCorrectToMultiline` or else-branch has multiple expressions). Wraps result in parens when parent is a binary operator (heuristic: scan source byte before node start). Branch swap on unless ternary; no swap on multiline (keyword preserved). Deferred (45 residuals): multi-stmt body preservation in multiline else, elsif chain multiline rendering, block-param `|` false-positive in operator detection (next-keyword case), yield/super/defined/not constructs needing inner paren-wrap.
- **Solo: Style/Lambda** (this commit) — +24 Style corrections. Two directions: literal `->[(args)] { body }` ↔ method `lambda { [|args|] body }`. Strips `(...)` or `|...|` from params source. Preserves spacing between selector and opening (special-case force-space when result would be `lambdado`). Deferred (6 residuals): numbered/it params (4), do/end → {/} delimiter swap when block is unparen-call arg (2).
- **Solo: Style/InfiniteLoop** (this commit) — +23 Style corrections. Block form `while/until LITERAL [do] body end` → `loop do body end` (replace keyword..cond/do header). Single-line modifier → `loop { body }`. Postloop `begin..end while/until LITERAL` → `loop do <inner> end` (uses begin/end keyword locations to splice the inner body). Deferred (2 residuals): multiline modifier with comment-preserving re-indent.
- **Solo: MemoizedInstanceVariableName** (last commit) — +26 Naming corrections. Replace ivar name range with `@<suggested>`. Both `||=` (single offense) and `defined?` patterns (3 offenses for return/defined/write).
- **Solo: Style/For** (this commit) — +25 Style corrections. For→Each (`for IDX in COLL [do]` → `COLL.each do |IDX|`) handles range/operator-method/and-or paren-wrap, safe-nav `&.each`, multi-target index. Each→For (`COLL.each do |IDX|` → `for IDX in COLL do`) handles explicit block params and missing-param `_` placeholder; skips numbered/it params (NumberedParametersNode location is not a usable param range in Prism).
- **Solo: Style/WordArray** (this commit) — +19 Style corrections. `[strings...]` → `%w(...)` / `%W(...)`. Use `string_content` (unescaped) per element; re-escape `\n` `\t` `\r` `\\` to backslash form. `%W` when any escape needed; `%w` otherwise. Single-line by default; multi-line preservation only when no escapes AND array source contains real newlines (avoids splitting embedded-newline content across lines). Skip elements containing space, `(`, or `)`. Deferred (~19 residuals): percent → bracketed direction (`%w(a b)` → `["a", "b"]`), partial-newline preservation, custom WordRegex / preferred-delimiter cases.
- **Cluster 5** (this commit) — 3 cops, +122 Style corrections. Style/BlockDelimiters (44/44), Style/GuardClause (48/48 incl. 8 heredoc), Style/ClassAndModuleChildren (30/30 incl. sibling-namespace pre-scan). Zero residuals across all three.
- **Solo: Style/SoleNestedConditional** (last commit) — +64 Style corrections. Merge nested `if` → `&&` / `||`. Handles paren-wrap for subscript-call `h[:a]` (Prism's `opening_loc()` returns `[`; check actual byte for `(`), block-method `ok? bar do...end` (skip parenthesize_method when block present), multiline `&&`/`||` continuation (preserve newline whitespace via AST-based op range), single-line `; end` separator preservation. Zero residuals.
- **Solo: Style/RedundantCondition** (last commit) — +53 Style corrections. Full RuboCop translation of `make_ternary_form` / `if_source` / `else_source` / `correct_ternary`. Covers ternary + block form, branches-have-method/assignment, without-arg-parens method wrap, range/rescue/modifier-form else-wrap, hash-bare brace-wrap, parent-is-call paren-wrap, comment-skip. Zero residuals.
- **Solo: Style/Next** (this commit) — +46 Style corrections. Convert trailing `if cond; body; end` inside iterator block to `next [unless] cond` + body. 3 residuals: multi-pass cases (nested-autocorrect re-indent overlap, misaligned-end inner-if cascade) — single-pass applier skips overlapping edits.
- **Solo: Style/IfUnlessModifierOfIfUnless** (this commit) — +1 correction. `correction_covered` tracking prevents inner modifier nodes from emitting overlapping corrections; outer node's recursive expansion handles full chain in one pass.
- **Cluster: Style/IfUnlessModifier + Style/IfUnlessModifierOfIfUnless** (last commit) — +61 Style corrections (~58 + ~3). Block↔modifier conversion: block-form `if cond; body; end` → `body if cond` (single-line modifier); modifier `body if cond` → block form when too long. Nested-modifier expansion for IfUnlessModifierOfIfUnless. Deferred (3 residuals): heredoc-arg edge case in IfUnlessModifier, implicit-match conditional flattening, nested-mofifier multiline edge.
- **Cluster: Layout indentation** (this commit) — 3 cops, +83 Layout corrections. CaseIndentation (~18): replace line-leading whitespace with `expected_col` spaces for each `when`/`in` keyword. FirstArrayElementIndentation (~21): same shape on first element + right bracket. IndentationConsistency (~21): multi-edit `reindent_node()` shifts every line of offending node by `delta = expected - actual`.
- **Solo: Style/AccessModifierDeclarations** (last commit) — +209 Style corrections. Group style: extract inline `private def foo` → bare `private` group at scope end; symbol list `private :foo, :bar` → moved to group; preceding-comment dedent. Inline style: bare `private` → `private def foo` prefix on each following def at same indent (handles `;` same-line and multi-def-group). `scope_end_offset` stored in `ModifierInfo`. Sibling collection: last offense corrects all siblings.
- **Cluster: Layout space-inside-braces** (this commit) — 2 cops, +40 Layout corrections. SpaceInsideHashLiteralBraces (+~20), SpaceInsideBlockBraces (+~20). Tiny insert/delete edits at brace boundaries; empty-brace `{}`↔`{ }` replace; `{|` block-pipe spacing.
- **Solo: Style/IfWithSemicolon + Style/OneLineConditional residual** (last commit) — +72 Style corrections. IfWithSemicolon (+27): `;` byte → `\n` for require-newline cases (multi-stmt or any masgn/block); replace whole node for ternary/elsif via `correct_elsif`. OneLineConditional residual (+45): full RuboCop `make_ternary_form` translation — multi-stmt else preservation, elsif recursion, parent-is-operator paren-wrap, IndentationWidth gating.
- **Solo: Style/QuotedSymbols** (last commit) — +23 Style corrections. Swap quote style on `:'X'` ↔ `:"X"` and hash-colon `'X':` ↔ `"X":`. Replace whole quoted body. Re-escape inside content: when going double→single, unescape `\"` → `"`; when single→double, unescape `\'` → `'`. Other escapes (`\\`, `\n`, etc.) pass through unchanged.
- **Solo: Style/SymbolProc** (last commit) — +33 Style corrections. Translates RuboCop `autocorrect_with_args` / `autocorrect_without_args`. Three shapes: (a) no args, no parens — replace ` { |x| x.foo }` with `(&:foo)`; (b) empty parens `(   )` — swallow `(...)` + block, replace with `(&:foo)`; (c) call has args — insert `, &:foo` after last arg (or ` &:foo` if trailing comma) and delete block. Deferred (5 residuals): lambda-literal `->` → `lambda(&:foo)`, super blocks.
- **Solo: LiteralAsCondition** (last commit) — +95 Lint corrections. Wired: block-form & modifier if/unless (replace whole node with appropriate branch source), ternary, while truthy → `true` / falsey → drop, until falsey → `false` / truthy → drop, postloop while/until (use begin-block inner statements as body), and/or with literal lhs → replace whole node with rhs (skip return/break/next rhs). Deferred (36 residuals): elsif chain rewrites (`if x; ...elsif literal; ...end` → `if x; ...else; ...end`), `if literal && literal_rhs` (multi-pass conflict between outer-if and and-node corrections).

**Known deferred edge cases** (not blocking cluster commits):

- `Style/RedundantBegin` — 9 mismatches: assignment-context comment/whitespace preservation.
- `Lint/SafeNavigationChain` — 8 mismatches: paren-wrap inside binary operands; `[]`/`[]=` index-method rewrites.
- `Layout/HeredocIndentation` — 0 failures. Fully wired: squiggly (re-indent body + closing), non-squiggly (rewrite `<<`/`<<-` → `<<~` + re-indent body + closing), squish (same as non-squiggly).
- `Layout/HashAlignment` — 1 failure: multi-pass table→key style regression in `prefer_table_when_least_offenses` test; single-pass corrector can't replicate RuboCop's iterative behavior.
- `Layout/BlockAlignment` — 1 failure: complex multi-offense chain case.
- `Layout/FirstHashElementIndentation` — 2 failures: edge cases TBD.
- `Layout/FirstArgumentIndentation` — 17 failures: nested-call `special_for_inner_method_call` style, multi-offense interactions.

### What's next — candidate clusters

Top unwired cops by failing-correction count (from `cargo test --test tester` strict mode):

| Count | Cop                              | Likely cluster shape                                                   |
| ----: | -------------------------------- | ---------------------------------------------------------------------- |
|   919 | Style/ConditionalAssignment      | branch-rewrite (lift assignment out of if/case) — own cluster, hardest |
|   209 | Style/AccessModifierDeclarations | move/group `private`/`protected` declarations                          |
|   131 | Lint/LiteralAsCondition          | replace literal cond with `true`/`false` body                          |
|    99 | Style/OneLineConditional         | one-line if → ternary                                                  |
|    76 | Layout/FirstArgumentIndentation  | re-indent (whitespace-only edits)                                      |
|    64 | Style/SoleNestedConditional      | merge nested `if` → `&&`/` \|\|`                                       |
|    63 | Lint/UselessAssignment           | delete dead assignment                                                 |
|    60 | Layout/HeredocIndentation        | re-indent heredoc body                                                 |
|    59 | Style/IfUnlessModifier           | wrap/unwrap modifier-if                                                |
|    56 | Layout/HashAlignment             | re-align hash keys (whitespace-only)                                   |

**Next-cluster candidates** (group by correction shape, not dept):

- **Cluster 3 — modifier-conditional rewrites**: `Style/IfUnlessModifier` (59) + `Style/Next` (49) + `Style/GuardClause` (48) + `Style/OneLineConditional` (99). All convert between block and modifier conditional forms.
- **Cluster 4 — Layout whitespace re-aligners**: `Layout/FirstArgumentIndentation` (76) + `Layout/HashAlignment` (56) + `Layout/HeredocIndentation` (60) + `Layout/RescueEnsureAlignment` (50) + `Layout/FirstHashElementIndentation` (31) + `Layout/BlockAlignment` (36) + `Layout/SpaceInsideArrayLiteralBrackets` (46) + `Layout/SpaceInsideReferenceBrackets` (31). All edit whitespace runs only.
- **Cluster 5 — dead-code removers**: `Lint/UselessAssignment` (63) + `Lint/LiteralAsCondition` (131). Delete or simplify based on liveness.
- **Solo big lifts**: `Style/ConditionalAssignment` (919) and `Style/AccessModifierDeclarations` (209) — too custom for cluster delegation; hand-wire one cop at a time.

## Production-readiness gaps

High cop count ≠ prod-ready. Gaps before drop-in RuboCop parity:

1. **Autocorrect coverage** — 7,710 / 11,217 corrections wired (69%). 157 cops still partial/unwired. Target ≥90%. **(active workstream)**
2. **CLI incomplete** — `--only`/`--except`, `-f json`/`-f emacs`, `--parallel` unchecked.
3. **Config edges** — `inherit_from`, `inherit_gem`, glob `Include`/`Exclude`, brace-expand partial. Fuzz against Rails/Discourse/Shopify `.rubocop.yml`.
4. **No real-world corpus** — 28k tests all from RuboCop specs. Run 3+ OSS codebases, diff vs RuboCop (target ±1% parity).
5. **Hard cops skipped** — Style/FormatString, Bundler/OrderedGems.
6. **Pending + Disabled** — 210 opt-in cops. Priority after enabled-default = 100%.
7. **No dogfooding** — not self-hosted; no CI lint on real Ruby.
8. **LSP unvalidated** — library API exists; no editor exercises E2E.
9. **No benchmarks** — "50-100x" target not measured. Need repro suite vs RuboCop.
10. **Not released** — no `cargo publish`, Homebrew formula, versioned binaries, 1.0 tag.

Stages: **alpha (internal)** → close 1/2/3 → **beta** → close 4/5/9/10 → **1.0**.

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

### Cluster-wiring strategy (multiple cops with the same correction shape)

When asked "what's next":

1. **Find candidates** — `cargo test --test tester 2>&1 | grep -oE '\[\w+/\w+\] \w+: TOML expects correction but cop emitted no Correction' | sort | uniq -c | sort -rn | head -30`. Top of list = highest-impact cops.
2. **Group by correction shape** — read each cop's TOML `corrected` blocks. Cops doing the same rewrite (delete keyword, swap operands, unwrap, insert prefix) → one cluster. Mixin sharing is a hint but **not** the criterion — what matters is the correction pattern, not the offense detection.
3. **Pick a template cop** — usually the simplest member. Wire it by hand. Reference templates already in tree:
   - `src/cops/style/even_odd.rs` — single `Correction::replace` over offense range.
   - `src/cops/style/yoda_condition.rs` — multi-edit swap with optional operator flip; demonstrates `Option<Location>` handling on `message_loc()`.
   - `src/cops/style/not.rs` — branching correction (flip / paren-wrap / paren-preserve / simple).
   - `src/cops/style/redundant_freeze.rs` — delete trailing call.
   - `src/cops/style/redundant_begin.rs` — multi-line unwrap with comment preservation.
4. **Delegate the tail to a Sonnet subagent** for mechanical members:
   ```
   Agent(subagent_type="general-purpose", model="sonnet",
         isolation="worktree", run_in_background=true, mode="bypassPermissions")
   ```
   Brief with: template file path, the cluster's TOML diffs, autocorrect API, **strict no-regression rule** (zero mismatches; revert anything causing regressions). Tell agent to run `cargo test --test tester` and report final mismatch count.
5. **Surgical merge** — agents may branch from stale main:
   - `git log --oneline {worktree-base}..main -- {cluster-files}` — confirm main hasn't moved on the same files. If clean, `cp` files over.
   - **Do NOT cherry-pick** agent's `lib.rs`/`cops/mod.rs`/`COPS.md` edits — those reflect stale state. Re-derive doc updates from current `cargo test` output.
   - `cargo test --test tester` must show fewer total errors and zero new failures.
   - `git worktree remove -f -f .claude/worktrees/{name}` + `git worktree prune` + `git branch -D {branch}`.
6. **Document deferred edge cases** in commit body — partial wiring is fine if 80%+ of the cluster's expected corrections land. Track residual mismatches in this CLAUDE.md "Current focus" section.
7. Commit: `feat(autocorrect): wire cluster N {pattern} corrections (M cops)`.

### Mandatory doc sync on every autocorrect commit

- **`COPS.md` line 8** — `Autocorrect progress: X / 11,217 (Y%)` and unwired-cop count.
- **`COPS.md` Summary table** — bump dept rows + Total row.
- Numbers source: `cargo test --test tester 2>&1 | grep "Corrections validated"` for total wired; per-dept failure delta for per-row counts. Total wired = 11,217 − (sum of correction failures across depts).
- This CLAUDE.md "Current focus" section — bump cluster log + total wired.
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
