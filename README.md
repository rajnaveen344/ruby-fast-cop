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
| Lint/AssignmentInCondition     | 69    | Partial |
| Lint/Debugger                  | 97    | Partial |
| Layout/LineLength              | 192   | Partial |
| Metrics/BlockLength            | 38    | Partial |
| Style/FormatStringToken        | 355   | Partial |
| Style/HashSyntax               | 189   | Partial |

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

- [ ] **More cops** - 11 of 631 implemented; focus on most-used cops first
- [ ] **Fix partial cops** - 6 implemented cops have failing tests from newly resolved test data
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
