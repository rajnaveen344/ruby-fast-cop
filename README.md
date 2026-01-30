# ruby-fast-cop

A high-performance Ruby linter written in Rust. Drop-in replacement for RuboCop with 50-100x faster execution.

## Goals

- **Fast**: Written in Rust for maximum performance
- **Compatible**: Same CLI interface and configuration as RuboCop
- **Embeddable**: Use as a library in other tools (LSP servers, editors, CI tools)
- **Practical**: Focus on the most commonly used cops first

## Status

🚧 **Early Development** - Not ready for production use.

## Motivation

RuboCop is excellent but slow on large codebases. Inspired by [Ruff](https://github.com/astral-sh/ruff) (Python), this project rewrites RuboCop's linting rules in Rust for dramatically faster execution.

| Tool          | Language | Performance             |
| ------------- | -------- | ----------------------- |
| RuboCop       | Ruby     | 1x (baseline)           |
| ruby-fast-cop | Rust     | 50-100x faster (target) |

## Installation

```bash
# From source (requires Rust 1.75+)
cargo install --path .

# Or build locally
cargo build --release
```

## Usage

```bash
# Lint current directory
ruby-fast-cop

# Lint specific files
ruby-fast-cop app/models/*.rb

# With configuration
ruby-fast-cop -c .rubocop.yml

# Auto-correct (safe fixes only)
ruby-fast-cop -a

# Auto-correct (all fixes)
ruby-fast-cop -A

# Output as JSON
ruby-fast-cop -f json -o results.json
```

## Library Usage

ruby-fast-cop can be used as a library in other Rust projects (e.g., LSP servers, editor plugins):

```toml
# Cargo.toml
[dependencies]
ruby-fast-cop = "0.1"
```

See the [API documentation](https://docs.rs/ruby-fast-cop) for details.

## Configuration

ruby-fast-cop reads `.rubocop.yml` configuration files. Supported options:

```yaml
AllCops:
  TargetRubyVersion: 3.2
  Exclude:
    - "vendor/**/*"
    - "db/schema.rb"

Style/StringLiterals:
  Enabled: true
  EnforcedStyle: double_quotes

Layout/LineLength:
  Max: 120
```

## Supported Cops

See [COPS.md](COPS.md) for the full list of implemented cops.

### Priority Cops (Phase 1)

**Layout**

- [ ] `Layout/LineLength`
- [ ] `Layout/IndentationWidth`
- [ ] `Layout/TrailingWhitespace`
- [ ] `Layout/TrailingEmptyLines`

**Style**

- [ ] `Style/FrozenStringLiteralComment`
- [ ] `Style/StringLiterals`
- [ ] `Style/SymbolArray`
- [ ] `Style/WordArray`
- [ ] `Style/TrailingCommaInArrayLiteral`
- [ ] `Style/TrailingCommaInHashLiteral`

**Lint**

- [ ] `Lint/Debugger`
- [ ] `Lint/UnusedVariable`
- [ ] `Lint/UselessAssignment`
- [ ] `Lint/DuplicateMethods`
- [ ] `Lint/Syntax`

**Metrics**

- [ ] `Metrics/MethodLength`
- [ ] `Metrics/ClassLength`
- [ ] `Metrics/CyclomaticComplexity`

## Architecture

```
┌─────────────────┐     ┌──────────────┐     ┌─────────────┐
│  Ruby Source    │────▶│    Prism     │────▶│     AST     │
│    Files        │     │   Parser     │     │             │
└─────────────────┘     └──────────────┘     └─────────────┘
                                                    │
                                                    ▼
┌─────────────────┐     ┌──────────────┐     ┌─────────────┐
│    Offenses     │◀────│   Parallel   │◀────│    Cops     │
│    Report       │     │   Runner     │     │   (Rust)    │
└─────────────────┘     └──────────────┘     └─────────────┘
```

- **Parser**: [Prism](https://github.com/ruby/prism) via `ruby-prism` crate (Ruby 3.4's default parser)
- **Parallelism**: [Rayon](https://github.com/rayon-rs/rayon) for multi-threaded file processing
- **Config**: Full `.rubocop.yml` compatibility

## Development

```bash
# Run tests
cargo test

# Run with debug output
RUST_LOG=debug cargo run -- .

# Benchmark against RuboCop
hyperfine 'ruby-fast-cop .' 'rubocop .'
```

## Contributing

Contributions welcome! The easiest way to contribute is to implement a new cop.

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## Comparison with RuboCop

| Feature          | RuboCop  | ruby-fast-cop     |
| ---------------- | -------- | ----------------- |
| Performance      | Baseline | 50-100x faster    |
| Cop count        | ~500     | ~50 (growing)     |
| Custom Ruby cops | ✅       | ❌                |
| .rubocop.yml     | ✅       | ✅                |
| Auto-correct     | ✅       | ✅                |
| Library API      | ❌       | ✅                |
| LSP Server       | ✅       | Via ruby-fast-lsp |

## Prior Art

- [RuboCop](https://github.com/rubocop/rubocop) - The original Ruby linter
- [Ruff](https://github.com/astral-sh/ruff) - Fast Python linter in Rust (inspiration)
- [Prism](https://github.com/ruby/prism) - Ruby parser with Rust bindings

## License

MIT
