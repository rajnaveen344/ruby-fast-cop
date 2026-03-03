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

RuboCop (v1.84.1) has **631 cops** across 10 departments. We have **631 TOML test fixtures** — one per cop — with **26,968 test cases** extracted from RuboCop's own RSpec suite.

| Department      | Cops | Test Cases |
| --------------- | ---- | ---------- |
| Style           | 287  | 13,105     |
| Lint            | 152  | 5,904      |
| Layout          | 100  | 4,600      |
| InternalAffairs | 38   | 474        |
| Naming          | 19   | 2,217      |
| Metrics         | 10   | 272        |
| Gemspec         | 10   | 192        |
| Bundler         | 7    | 94         |
| Security        | 7    | 102        |
| Migration       | 1    | 8          |

### Implemented Cops (11 of 631)

| Cop                            | Tests | Status  |
| ------------------------------ | ----- | ------- |
| Style/RaiseArgs                | 35    | Passing |
| Style/AutoResourceCleanup      | 7     | Passing |
| Style/MethodCalledOnDoEndBlock | 10    | Passing |
| Style/RescueStandardError      | 37    | Passing |
| Style/StringMethods            | 2     | Passing |
| Style/FormatStringToken        | 355   | Passing |
| Style/HashSyntax               | 189   | Passing |
| Lint/AssignmentInCondition     | 69    | Passing |
| Lint/Debugger                  | 97    | Passing |
| Layout/LineLength              | 192   | Passing |
| Metrics/BlockLength            | 38    | Passing |

### Implementation Roadmap

RuboCop enables **most cops by default**. A `.rubocop.yml` file only overrides specific settings — all unmentioned cops run with their defaults. This means implementing the ~50 most commonly triggered default cops covers the vast majority of real-world usage.

Frequency data sourced from the [300 Days of RuboCop](https://lovro-bikic.github.io/300-days-of-rubocop/) study (~3,000 CI runs on a large Rails codebase). Full list of all 606 cops: **[COPS.md](COPS.md)**.

#### Tier 1 — Quick Wins (line/token-based, minimal AST)

| Cop                              | Difficulty | Real-World Frequency                              |
| -------------------------------- | ---------- | ------------------------------------------------- |
| Layout/TrailingWhitespace        | Easy       | #4 — 297 violations                               |
| Layout/TrailingEmptyLines        | Easy       | #5 — 227 violations                               |
| Style/FrozenStringLiteralComment | Easy       | #3 — 364 violations                               |
| Style/StringLiterals             | Easy       | Fires on every string literal                     |
| Layout/SpaceAfterComma           | Easy       | Very common formatting cop                        |
| Layout/LeadingCommentSpace       | Easy       | Common, simple regex                              |
| Style/Semicolon                  | Easy       | Simple token scan                                 |
| Style/NumericLiterals            | Easy       | Check integer underscores                         |
| Metrics/MethodLength             | Easy       | Constant in legacy code (reuse BlockLength logic) |
| Metrics/ClassLength              | Easy       | Constant in legacy code (reuse BlockLength logic) |

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

- [ ] **More cops** - 11 of 631 implemented; see [Implementation Roadmap](#implementation-roadmap) for priority list
- [ ] **Auto-correct** - Implement `-a` (safe) and `-A` (all) correction flags
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
| Cop count        | 631             | 11 implemented          |
| Test coverage    | ~27k test cases | 631 fixtures (1:1)      |
| Custom Ruby cops | Yes             | No                      |
| .rubocop.yml     | Yes             | Yes                     |
| Auto-correct     | Yes             | Planned                 |
| Library API      | Limited         | Yes                     |
| inherit_from     | Yes             | Yes                     |
| inherit_gem      | Yes             | Yes                     |

## License

MIT

## Acknowledgments

- [RuboCop](https://rubocop.org/) - The original Ruby linter
- [Prism](https://github.com/ruby/prism) - Ruby's new default parser
- [Ruff](https://github.com/astral-sh/ruff) - Inspiration for this project
