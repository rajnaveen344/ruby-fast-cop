# ruby-fast-cop

A high-performance Ruby linter written in Rust, designed as a drop-in replacement for [RuboCop](https://rubocop.org/). Inspired by [Ruff](https://github.com/astral-sh/ruff) for Python.

## Why?

RuboCop is powerful but slow. On large codebases, linting can take 30+ seconds. ruby-fast-cop aims to be **faster** by rewriting cops in Rust while maintaining full compatibility with `.rubocop.yml` configuration.

## Installation

```bash
# From source (requires Rust 1.75+)
git clone https://github.com/example/ruby-fast-cop
cd ruby-fast-cop
cargo build --release

# Binary will be at target/release/ruby-fast-cop
```

## Usage

```bash
# Lint current directory (uses .rubocop.yml if present)
ruby-fast-cop

# Lint specific files or directories
ruby-fast-cop app/ lib/ spec/

# Use a specific config file
ruby-fast-cop -c .rubocop.yml .

# Suppress warnings about unsupported cops
ruby-fast-cop --no-warnings

# Show version
ruby-fast-cop --version
```

## Configuration

ruby-fast-cop reads standard `.rubocop.yml` files:

```yaml
AllCops:
  TargetRubyVersion: 3.2
  Exclude:
    - "vendor/**/*"
    - "db/schema.rb"

Style/HashSyntax:
  EnforcedStyle: ruby19_no_mixed_keys

Metrics/BlockLength:
  Max: 50
  Exclude:
    - "spec/**/*_spec.rb"
    - "{db,config}/**/*" # Brace expansion supported

Layout/LineLength:
  Max: 120

Style/Documentation:
  Enabled: false # Per-cop enable/disable
```

### Supported Config Features

| Feature                       | Status    |
| ----------------------------- | --------- |
| `inherit_from` (files)        | Supported |
| `inherit_gem` (gems)          | Supported |
| `AllCops.Exclude`             | Supported |
| `AllCops.TargetRubyVersion`   | Supported |
| Per-cop `Enabled`             | Supported |
| Per-cop `Exclude` / `Include` | Supported |
| `EnforcedStyle`               | Supported |
| `Max` (metrics)               | Supported |
| Glob patterns with `{a,b}`    | Supported |

## Cop Coverage

RuboCop (v1.85.0) has **606 user-facing cops** across 9 departments. We have **606 TOML test fixtures** — one per cop — with **28,075 test cases** extracted from RuboCop's own RSpec suite.

### Implemented Cops (22 of 606 — all passing)

| Cop                              | Tests | Status  |
| -------------------------------- | ----- | ------- |
| Layout/LeadingCommentSpace       | 25    | Passing |
| Layout/LineLength                | 192   | Passing |
| Layout/SpaceAfterComma           | 9     | Passing |
| Layout/TrailingEmptyLines        | 10    | Passing |
| Layout/TrailingWhitespace        | 15    | Passing |
| Lint/AssignmentInCondition       | 69    | Passing |
| Lint/Debugger                    | 97    | Passing |
| Lint/LiteralInInterpolation      | 378   | Passing |
| Metrics/BlockLength              | 38    | Passing |
| Metrics/ClassLength              | 23    | Passing |
| Metrics/MethodLength             | 30    | Passing |
| Style/AutoResourceCleanup        | 7     | Passing |
| Style/FormatStringToken          | 355   | Passing |
| Style/FrozenStringLiteralComment | 25    | Passing |
| Style/HashSyntax                 | 189   | Passing |
| Style/MethodCalledOnDoEndBlock   | 10    | Passing |
| Style/NumericLiterals            | 25    | Passing |
| Style/RaiseArgs                  | 35    | Passing |
| Style/RescueStandardError        | 37    | Passing |
| Style/Semicolon                  | 24    | Passing |
| Style/StringLiterals             | 48    | Passing |
| Style/StringMethods              | 2     | Passing |

### Implementation Roadmap

RuboCop enables **most cops by default**. A `.rubocop.yml` file only overrides specific settings — all unmentioned cops run with their defaults. This means implementing the ~50 most commonly triggered default cops covers the vast majority of real-world usage.

Frequency data sourced from the [300 Days of RuboCop](https://lovro-bikic.github.io/300-days-of-rubocop/) study (~3,000 CI runs on a large Rails codebase). Full list of all 606 cops: **[COPS.md](COPS.md)**.

#### Tier 2 — High Impact (AST-based, moderate complexity)

| Cop                                 | Difficulty  | Real-World Frequency                              |
| ----------------------------------- | ----------- | ------------------------------------------------- |
| Style/RedundantReturn               | Medium      | Very common, check last expression                |
| Style/SymbolProc                    | Medium      | Common Ruby idiom (83 tests)                      |
| Style/MutableConstant               | Medium      | Very common (354 tests, largest Style fixture)    |
| Style/TrailingCommaInArrayLiteral   | Medium      | Part of #6 trailing comma family — 148 violations |
| Style/TrailingCommaInHashLiteral    | Medium      | #9 — 57 violations                                |
| Style/RedundantSelf                 | Medium      | Very common, self. in method body                 |
| Style/NegatedIf                     | Medium      | `unless` vs `if !`                                |
| Lint/UselessAssignment              | Medium-Hard | High value error-finding cop (149 tests)          |
| Lint/UnusedMethodArgument           | Medium      | Unused method args (41 tests)                     |
| Lint/UnusedBlockArgument            | Medium      | Unused block args (30 tests)                      |
| Lint/RedundantStringCoercion        | Medium      | `.to_s` inside interpolation                      |
| Layout/IndentationConsistency       | Medium      | #14 — 43 violations                               |
| Layout/SpaceAroundOperators         | Medium      | Common formatting (99 tests)                      |
| Layout/SpaceInsideHashLiteralBraces | Medium      | Common formatting (40 tests)                      |
| Layout/SpaceInsideBlockBraces       | Medium      | Common formatting (43 tests)                      |
| Layout/EmptyLineBetweenDefs         | Medium      | Common formatting                                 |
| Layout/FirstHashElementIndentation  | Medium      | #13 — 48 violations                               |
| Naming/MethodName                   | Medium      | snake_case methods (239 tests)                    |
| Naming/VariableName                 | Medium      | snake_case variables (118 tests)                  |

#### Tier 3 — Complex but Valuable

| Cop                                   | Difficulty | Real-World Frequency                        |
| ------------------------------------- | ---------- | ------------------------------------------- |
| Layout/MultilineMethodCallIndentation | Hard       | **#1 — 486 violations** (most violated cop) |
| Layout/IndentationWidth               | Hard       | Foundational (177 tests)                    |
| Layout/HashAlignment                  | Hard       | Very common in legacy (131 tests)           |
| Layout/EndAlignment                   | Hard       | Keyword/variable alignment (207 tests)      |
| Style/BlockDelimiters                 | Hard       | do/end vs `{}` rules (173 tests)            |
| Style/GuardClause                     | Hard       | Control flow analysis (91 tests)            |
| Style/IfUnlessModifier                | Medium     | #11 — 53 violations (126 tests)             |
| Lint/UselessAccessModifier            | Hard       | Scope tracking (198 tests)                  |
| Metrics/AbcSize                       | Medium     | Assignment/Branch/Condition counting        |
| Metrics/CyclomaticComplexity          | Medium     | Decision point counting                     |

## Autocorrect

ruby-fast-cop supports autocorrect via `-a` (safe) and `-A` (all) flags:

```bash
# Safe autocorrect only
ruby-fast-cop -a .

# All autocorrect (safe + unsafe)
ruby-fast-cop -A .
```

16 of 22 implemented cops support autocorrect (716 correction tests passing).

### How Autocorrect Works: RuboCop vs Ruff vs ruby-fast-cop

All three tools find style violations then **rewrite the source file** with fixes applied. The key difference is _how_ they apply multiple edits and handle cascading fixes (where one cop's fix creates a new violation for another cop).

#### RuboCop — Iterative Re-Parse (up to 200 passes)

```
source.rb → Parse → Run cops → Apply fixes → Write back
               ↑                                  │
               └──── re-read corrected file ──────┘
                     (repeat up to 200 times)
```

RuboCop re-parses the **entire file from scratch** each pass. Safe but slow — it invokes Ruby's parser up to 200 times per file. Edits are applied **end-to-start** (descending offset order) so byte positions stay valid within a single pass.

#### Ruff — Smart Single-Pass + Safety Net (up to 10 passes)

```
source.py → Parse → Run rules → Forward-walk apply → Changed?
               ↑                                        │
               └──── re-parse (only if changed) ────────┘
                     (max 10 iterations, cycle detection)
```

Ruff's **forward-walk** algorithm sorts edits ascending by offset and walks forward with a cursor. Unchanged gaps are copied verbatim, replacements are spliced in, and overlapping edits are skipped (caught in the next pass). Most files need only 1 pass.

#### ruby-fast-cop — Follows Ruff's Model

```
source.rb → Parse (Prism) → Run cops → Forward-walk apply
               ↑                              │
               └──── re-parse if changed ─────┘
                     max 10 iterations
                     cycle detection (source hash)
                     write file ONCE at end
```

We use the same forward-walk algorithm as Ruff with three safety mechanisms:

- **Max 10 iterations** — can't loop forever
- **Cycle detection** — if we see the same source hash twice, stop (two cops fighting each other)
- **No-change detection** — if edits produce identical output, stop immediately

Example of cascading fixes in a single run:

```
Pass 1: StringLiterals fixes quotes     →  x = "hello"  →  x = 'hello'
        SpaceAfterComma adds spaces     →  [1,2,3]      →  [1, 2, 3]
Pass 2: FrozenStringLiteralComment      →  (adds # frozen_string_literal: true)
Pass 3: No more corrections             →  STOP, write file once
```

|                  | RuboCop       | Ruff         | ruby-fast-cop |
| ---------------- | ------------- | ------------ | ------------- |
| Language         | Ruby          | Rust         | Rust          |
| Max iterations   | 200           | 10           | 10            |
| Edit strategy    | end-to-start  | forward-walk | forward-walk  |
| File writes      | every pass    | once at end  | once at end   |
| Overlap handling | re-parse      | skip + retry | skip + retry  |
| Cycle detection  | no (just cap) | yes (hash)   | yes (hash)    |

## Library Usage

ruby-fast-cop can be embedded in other tools (LSP servers, editors, CI):

```rust
use ruby_fast_cop::{check_source, check_file_with_config, Config};
use std::path::Path;

// Quick check with default cops
let offenses = check_source("puts 'hello'", "test.rb");

// Check with configuration
let config = Config::load_from_file(Path::new(".rubocop.yml")).unwrap();
let offenses = check_file_with_config(Path::new("app/models/user.rb"), &config).unwrap();

for offense in offenses {
    println!("{}", offense);
}
```

## Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                         .rubocop.yml                                │
└─────────────────────────────────────────────────────────────────────┘
                                 │
                                 ▼
┌─────────────────────────────────────────────────────────────────────┐
│                        Config Parser                                │
│  - YAML parsing with serde                                          │
│  - inherit_from / inherit_gem resolution                            │
│  - Glob pattern matching (globset)                                  │
└─────────────────────────────────────────────────────────────────────┘
                                 │
                                 ▼
┌─────────────────────────────────────────────────────────────────────┐
│                        Prism Parser                                 │
│  - Ruby 3.4's default parser (ruby-prism crate)                     │
│  - Error-tolerant (can lint files with syntax errors)               │
│  - Supports Ruby 2.5+ syntax                                        │
└─────────────────────────────────────────────────────────────────────┘
                                 │
                                 ▼
┌─────────────────────────────────────────────────────────────────────┐
│                   Single-Pass AST Traversal                         │
│  - Visitor pattern (CopRunner)                                      │
│  - All enabled cops notified per node type                          │
│  - Offenses collected in source order                               │
└─────────────────────────────────────────────────────────────────────┘
                                 │
                                 ▼
┌─────────────────────────────────────────────────────────────────────┐
│                         Formatter                                   │
│  - Progress (default) - dots with summary                           │
└─────────────────────────────────────────────────────────────────────┘
```

## Development

```bash
# Run tests
cargo test

# Run the linter
cargo run -- src/

# Check test fixture statistics
cargo run --bin fixture_stats

# Re-sync test fixtures from RuboCop specs
/rubocop-test-importer sync
```

## TODO

### High Priority

- [ ] **More cops** - 22 of 606 implemented; see [Implementation Roadmap](#implementation-roadmap) for priority list
- [x] **Auto-correct** - `-a` (safe) and `-A` (all) flags with Ruff-style iterative correction
- [ ] **Parallel processing** - Use rayon for multi-threaded file processing

### Medium Priority

- [ ] **JSON formatter** - `-f json` for CI integration
- [ ] **`--only` / `--except` flags** - Filter cops at runtime

### Low Priority

- [ ] **Emacs formatter** - `-f emacs` output format
- [ ] **DisplayStyleGuide** - Show style guide URLs in offense messages
- [ ] **Cache** - Probably not needed (already fast enough)

### Won't Implement

- **Custom Ruby cops** - `require:` directive for loading Ruby extensions
- Use RuboCop directly for custom cops, or port them to Rust

## Comparison with RuboCop

| Feature          | RuboCop         | ruby-fast-cop           |
| ---------------- | --------------- | ----------------------- |
| Performance      | Baseline        | 50-100x faster (target) |
| Cop count        | 606             | 22 implemented          |
| Test coverage    | ~28k test cases | 606 fixtures (1:1)      |
| Custom Ruby cops | Yes             | No                      |
| .rubocop.yml     | Yes             | Yes                     |
| Auto-correct     | Yes             | Yes (16 of 22 cops)     |
| Library API      | Limited         | Yes                     |
| inherit_from     | Yes             | Yes                     |
| inherit_gem      | Yes             | Yes                     |

## License

MIT

## Acknowledgments

- [RuboCop](https://rubocop.org/) - The original Ruby linter
- [Prism](https://github.com/ruby/prism) - Ruby's new default parser
- [Ruff](https://github.com/astral-sh/ruff) - Inspiration for this project
