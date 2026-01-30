# CLAUDE.md

Instructions for Claude when working on this project.

## Project Overview

ruby-fast-cop is a high-performance Ruby linter written in Rust, designed as a drop-in replacement for RuboCop. The goal is 50-100x faster linting by rewriting cops in Rust, similar to how Ruff replaced Python linters.

## Key Design Decisions

### Parser: Prism (ruby-prism crate)
- Use `ruby-prism = "1.9.0"` for parsing Ruby source code
- Prism is Ruby 3.4's default parser, future-proof choice
- Prism can parse all Ruby versions (2.5+) - syntax is mostly additive
- Provides error-tolerant parsing (can lint files with syntax errors)
- Note: Prism `Location` only provides byte offsets, not line/column - we compute those from source

### Architecture
- **No Ruby runtime** - All cops are pure Rust, no extraction or WASM
- **RuboCop compatible** - Same CLI flags, same config format, same cop names
- **Parallel by default** - Use Rayon for multi-threaded file processing
- **Library-first** - Core functionality exposed as a library; CLI is a thin wrapper
- **Embeddable** - Designed for integration into LSP servers, editors, and other tools

### Cop Implementation Strategy
- Rewrite cops in Rust (like Ruff did for Python)
- Don't try to extract or translate Ruby cops automatically
- Focus on most-used cops first (~50-100 covers 90% of real usage)

## Project Structure

```
src/
├── lib.rs               # Library entry point (public API)
├── main.rs              # CLI entry point (thin wrapper around lib)
├── config/
│   ├── mod.rs           # .rubocop.yml parser
│   └── types.rs         # Config structs
├── parser/
│   └── mod.rs           # Prism wrapper
├── cops/
│   ├── mod.rs           # Cop trait + registry
│   ├── lint/            # Lint/* cops
│   ├── style/           # Style/* cops
│   ├── layout/          # Layout/* cops
│   ├── metrics/         # Metrics/* cops
│   └── naming/          # Naming/* cops
├── runner/
│   └── mod.rs           # Parallel file processing
├── offense.rs           # Offense type (public)
└── formatters/
    ├── mod.rs           # Formatter trait
    ├── json.rs          # JSON output
    ├── progress.rs      # Progress dots (default)
    └── emacs.rs         # Emacs-compatible output
```


## Key Dependencies

```toml
[dependencies]
ruby-prism = "1.9.0"     # Ruby parser (Prism)
thiserror = "2"          # Error handling
clap = { version = "4", features = ["derive"] }  # CLI (TODO)
serde = { version = "1", features = ["derive"] } # Serialization (TODO)
serde_yaml = "0.9"       # .rubocop.yml parsing (TODO)
rayon = "1.8"            # Parallel processing (TODO)
```

## Implementing a Cop

Each cop should:
1. Live in the appropriate department directory (`cops/lint/`, `cops/style/`, etc.)
2. Implement the `Cop` trait
3. Register itself in the cop registry
4. Support configuration from `.rubocop.yml`

Example cop structure:
```rust
use crate::cops::{Cop, Offense, Severity};
use ruby_prism::Node;

pub struct Debugger {
    enabled: bool,
}

impl Cop for Debugger {
    fn name(&self) -> &'static str {
        "Lint/Debugger"
    }

    fn check(&self, node: &Node, source: &str) -> Vec<Offense> {
        // Implementation
    }
}
```

## CLI Compatibility

Must support these RuboCop flags:
- `-c, --config FILE` - Config file path
- `-f, --format FORMATTER` - Output format (progress, json, emacs, etc.)
- `-o, --out FILE` - Output file
- `-a, --autocorrect` - Safe auto-correct
- `-A, --autocorrect-all` - All auto-correct
- `--only COP1,COP2` - Run specific cops
- `--except COP1,COP2` - Exclude cops
- `-l, --lint` - Only Lint/* cops
- `--parallel` / `--no-parallel` - Parallel processing

Exit codes:
- 0: No offenses
- 1: Offenses found
- 2: Error

## Config File Format

Support standard `.rubocop.yml`:
```yaml
AllCops:
  TargetRubyVersion: 3.2
  Exclude:
    - 'vendor/**/*'

Style/StringLiterals:
  Enabled: true
  EnforcedStyle: double_quotes
```

Must handle:
- `inherit_from` (file inheritance)
- `inherit_gem` (gem-based config)
- Glob patterns in `Include`/`Exclude`
- Per-cop configuration

## Output Formats

### Progress (default)
```
Inspecting 100 files
....F...W....

Offenses:
app/models/user.rb:10:5: C: Style/StringLiterals: Prefer double quotes.
```

### JSON
```json
{
  "files": [...],
  "summary": {
    "offense_count": 5,
    "target_file_count": 100,
    "inspected_file_count": 100
  }
}
```

## Testing

- Unit tests for each cop with Ruby code snippets
- Integration tests comparing output to RuboCop
- Benchmark tests for performance validation

## Performance Targets

- Parse 1000 files: < 1 second
- Lint 1000 files (common cops): < 2 seconds
- Should be 50-100x faster than RuboCop

## Common Tasks

### Adding a new cop
1. Create file in `src/cops/{department}/{cop_name}.rs`
2. Implement `Cop` trait
3. Add to department's `mod.rs`
4. Register in cop registry
5. Add tests
6. Update COPS.md

### Adding a new formatter
1. Create file in `src/formatters/{name}.rs`
2. Implement `Formatter` trait
3. Register in formatter factory
4. Add CLI flag support

## Library API

This crate is both a CLI binary and a library. The library API allows embedding in other tools like `ruby-fast-lsp`.

Key design principles:
- Keep public API minimal and stable
- Expose functions to check source code directly (not just files)
- Make core types serializable (serde) for easy integration
- Avoid exposing internal types (AST nodes, parser details)

The CLI (`main.rs`) should be a thin wrapper around the library (`lib.rs`).

## References

- [RuboCop docs](https://docs.rubocop.org/rubocop/)
- [Prism Ruby parser](https://github.com/ruby/prism)
- [ruby-prism crate](https://crates.io/crates/ruby-prism)
- [Ruff (inspiration)](https://github.com/astral-sh/ruff)
