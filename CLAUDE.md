# CLAUDE.md

Instructions for Claude when working on this project.

## Communication Mode

**Default: `/caveman ultra`.** Drop articles, filler, hedging. Abbreviate (DB/auth/config/req/res/fn/impl). Arrows for causality (X → Y). Fragments OK.

Exceptions — drop caveman temporarily: security warnings, destructive-op confirms, multi-step sequences where fragment order risks misread, user asks to clarify.

**Never cavemanize:** code, commit messages, PR descriptions, TOML fixtures, error strings.

**Off switch:** "stop caveman" / "normal mode". Subagents get explicit `/caveman ultra` in prompt.

## Project Overview

ruby-fast-cop = Rust port of RuboCop. Target 50-100x faster (like Ruff:Python).

**State:** 443/606 cops (396/396 enabled-by-default = 100%; 37/149 pending-by-default). ~28,075 test cases from RuboCop v1.85.0 RSpec, all green.

> **Architecture:** see [`ARCHITECTURE.md`](./ARCHITECTURE.md) for runtime shape, registration, autocorrect pipeline, testing pipeline. CLAUDE.md = conventions; ARCHITECTURE.md = structure. Update ARCHITECTURE.md only when runtime/registration/autocorrect/testing shape changes.

## Deferred pending-by-default cops (0)

All previously deferred cops cleared. `Style/ArgumentsForwarding` ported in `src/cops/style/arguments_forwarding.rs` (187/187 fixture tests green).

## Production-readiness gaps

High cop count ≠ prod-ready. Gaps before drop-in RuboCop parity:

1. **Autocorrect coverage** — ~24/395 cops emit `Correction`. Target ≥90%.
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

## Cop impl strategy

- **Translate from RuboCop source**, don't reinvent. Battle-tested edge cases.
- Fetch: `https://raw.githubusercontent.com/rubocop/rubocop/master/lib/rubocop/cop/{dept}/{name}.rb` + mixins.
- 100-line Ruby cop → ~150-250 LOC Rust. Not 500+. Match RuboCop structure.
- Shared mixin (e.g. VariableForce) → mirror file structure in `src/helpers/{mixin}/`. No monoliths.
- **Never hardcode fixes to pass specific tests.** Understand RuboCop behavior first, implement generally.

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

### Adding a cop

1. Fetch RuboCop source (+ mixins, + VariableForce-style shared modules if referenced).
2. Read `tests/fixtures/{dept}/{cop}.toml`. Spot-check vs RuboCop spec if suspicious: `curl -s "https://raw.githubusercontent.com/rubocop/rubocop/master/spec/rubocop/cop/{dept}/{cop}_spec.rb"`.
3. Create `src/cops/{dept}/{cop}.rs`. Implement `Cop` trait — translate, don't reinvent.
4. Add `mod` + `pub use` in `src/cops/{dept}/mod.rs`.
5. Append `register_cop!` at bottom of cop file. **Only** registration step.
6. Set `implemented = true` in TOML.
7. `cargo test --test tester`. If fails, compare with spec, fix impl (not test).
8. Run `/cop-review` — compares vs Ruby source, flags complexity. Fix before moving on.
9. Update README.md (impl table), COPS.md (status + counts), CLAUDE.md (cop count). ARCHITECTURE.md only if runtime shape changed.

Example cop:
```rust
use crate::cops::{Cop, CheckContext, Offense, Severity};
use crate::node_name;

#[derive(Default)]
pub struct Debugger;

impl Debugger {
    pub fn new() -> Self { Self }
}

impl Cop for Debugger {
    fn name(&self) -> &'static str { "Lint/Debugger" }

    fn check_call(&self, node: &ruby_prism::CallNode, ctx: &CheckContext) -> Vec<Offense> {
        let _method = node_name!(node);
        vec![]
    }
}

crate::register_cop!("Lint/Debugger", |_cfg| Some(Box::new(Debugger::new())));
```

### Implementing many cops — mixin-cluster strategy

When asked "what's next":

1. **Find candidates**: TOML `implemented = false`, cross-checked vs COPS.md's "Enabled by Default" per-dept `### Enabled by Default` subsections (H3, not H2 — naive regex bleeds into Pending/Disabled).

   ```python
   python3 << 'PYEOF'
   import re, glob, os
   with open('COPS.md') as f: cops_md = f.read()
   sections = re.findall(r'### Enabled by Default.*?\n(.*?)(?=^##+ |\Z)', cops_md, re.DOTALL | re.MULTILINE)
   enabled = set()
   for sec in sections:
       enabled.update(re.findall(r'\b([A-Z][a-zA-Z]+/[A-Z][a-zA-Z0-9]+)\b', sec))
   # sanity: ~396 cops across 9 depts for v1.85.0
   candidates = []
   for f in glob.glob('tests/fixtures/**/*.toml', recursive=True):
       c = open(f).read()
       m = re.search(r'cop = "(.+?)"', c)
       if not m or m.group(1) not in enabled or 'implemented = true' in c: continue
       cop = m.group(1)
       dept = cop.split('/')[0].lower()
       name = re.sub(r'(?<!^)(?=[A-Z])', '_', cop.split('/')[1]).lower()
       if os.path.exists(f'src/cops/{dept}/{name}.rs'):
           print(f'WARN: {cop} has .rs but TOML false'); continue
       candidates.append((len(re.findall(r'\[\[tests\]\]', c)), cop))
   candidates.sort(reverse=True)
   for t, n in candidates[:30]: print(f'{t:>5}  {n}')
   PYEOF
   ```

2. **Cluster by shared mixin** — fetch Ruby source, check `include`. Cops sharing a mixin (SurroundingSpace, EndKeywordAlignment, PrecedingFollowingAlignment) → one cluster.
3. **One subagent per cluster** — builds shared helper first, then cluster cops.
4. **Reference existing similar cop** when briefing — e.g. `src/cops/style/redundant_freeze.rs` (simple check_call), `src/cops/lint/redundant_safe_navigation.rs` (visitor + scope).
5. **Assess difficulty** Easy/Medium/Hard — Ruby LOC + mixin LOC + config + AST surface.
6. **Launch with worktree isolation:**
   ```
   Agent(subagent_type="general-purpose", isolation="worktree", run_in_background=true, mode="bypassPermissions")
   ```
7. **Surgical merge** after agent completes:
   - Copy new `.rs` + TOMLs from worktree
   - Manually add `mod` + `pub use` in dept `mod.rs`
   - Verify `register_cop!` in each cop file (add if agent used old-style match arms)
   - **Do NOT cherry-pick** agent's lib.rs/cops/mod.rs edits — agent may have branched from stale main using the old match-arm registration style. Only the register_cop! macro is canonical.
   - `cargo test --test tester` must be 100% green
   - `rm -rf .claude/worktrees/` + `git worktree prune` + delete branches
8. Commit: `feat({dept}): implement N {cluster} cops (cluster)`.

### Fixing a partial cop

```bash
cargo test --test tester 2>&1 | grep "Failures in.*{cop}"
```
Read failing tests → compare RuboCop spec → fix impl (or update TOML if extractor bug) → re-run.

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
