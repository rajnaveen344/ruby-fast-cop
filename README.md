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

## Implemented Cops

### Verified (passing all RuboCop parity tests)

- **Style/AutoResourceCleanup** - Checks for resources opened without block form
- **Style/FormatStringToken** - Checks format string token style (annotated/template/unannotated)
- **Style/MethodCalledOnDoEndBlock** - Checks for methods called on do...end blocks
- **Style/RaiseArgs** - Checks style of raise/fail arguments (explode/compact)
- **Style/RescueStandardError** - Checks rescue StandardError style (explicit/implicit)
- **Style/StringMethods** - Checks for preferred string methods (intern vs to_sym)

### Partial Implementation (code exists, needs more config options)

- **Lint/AssignmentInCondition** - Checks for assignments in conditions
- **Lint/Debugger** - Checks for debugger calls (needs DebuggerMethods config)
- **Layout/LineLength** - Checks line length (needs tab width, AllowURI)
- **Metrics/BlockLength** - Checks block length (needs AllowedMethods, CountAsOne)
- **Style/HashSyntax** - Checks hash literal syntax (needs hash value omission support)

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

# Extract tests from RuboCop specs (for adding new cops)
cargo run --bin extract-rubocop-tests -- /path/to/rubocop/spec/rubocop/cop
```

## TODO

### High Priority

- [ ] **Auto-correct** - Implement `-a` (safe) and `-A` (all) correction flags
- [ ] **Parallel processing** - Use rayon for multi-threaded file processing
- [ ] **Verify more cops** - Mark implemented cops as passing tests

### Medium Priority

- [ ] **JSON formatter** - `-f json` for CI integration
- [ ] **`--only` / `--except` flags** - Filter cops at runtime
- [ ] **More cops** - Style/StringLiterals, Style/FrozenStringLiteralComment, etc.

### Low Priority

- [ ] **Emacs formatter** - `-f emacs` output format
- [ ] **DisplayStyleGuide** - Show style guide URLs in offense messages
- [ ] **Cache** - Probably not needed (already fast enough)

### Won't Implement

- **Custom Ruby cops** - `require:` directive for loading Ruby extensions
- Use RuboCop directly for custom cops, or port them to Rust

## Comparison with RuboCop

| Feature          | RuboCop  | ruby-fast-cop           |
| ---------------- | -------- | ----------------------- |
| Performance      | Baseline | 50-100x faster (target) |
| Cop count        | ~500     | 6 verified, 5 partial   |
| Custom Ruby cops | Yes      | No                      |
| .rubocop.yml     | Yes      | Yes                     |
| Auto-correct     | Yes      | Planned                 |
| Library API      | Limited  | Yes                     |
| inherit_from     | Yes      | Yes                     |
| inherit_gem      | Yes      | Yes                     |

## License

MIT

## Acknowledgments

- [RuboCop](https://rubocop.org/) - The original Ruby linter
- [Prism](https://github.com/ruby/prism) - Ruby's new default parser
- [Ruff](https://github.com/astral-sh/ruff) - Inspiration for this project
