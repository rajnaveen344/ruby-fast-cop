# CLAUDE.md

Instructions for Claude when working on this project.

## Project Overview

ruby-fast-cop is a high-performance Ruby linter written in Rust, designed as a drop-in replacement for RuboCop. The goal is 50-100x faster linting by rewriting cops in Rust, similar to how Ruff replaced Python linters.

**Current state:** 21 of 606 cops implemented (all passing), 606 TOML test fixtures with ~28,075 test cases extracted from RuboCop v1.85.0's RSpec suite.

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
- **Never hardcode fixes to pass specific test cases.** Always understand the underlying RuboCop behavior and implement the general solution. If a test fails, study the original RuboCop cop source to understand *why* it behaves that way, then replicate that logic broadly.

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
tests/
├── tester.rs            # Data-driven parity test runner
└── fixtures/            # TOML test fixtures (1 per cop, 606 total)
    ├── lint/
    ├── style/
    ├── layout/
    ├── metrics/
    ├── naming/
    ├── bundler/
    ├── gemspec/
    ├── security/
    └── migration/
```

## Key Dependencies

```toml
[dependencies]
ruby-prism = "1.9.0"     # Ruby parser (Prism)
thiserror = "2"          # Error handling
clap = { version = "4", features = ["derive"] }  # CLI
serde = { version = "1", features = ["derive"] } # Serialization
serde_yaml = "0.9"       # .rubocop.yml parsing
toml = "0.8"             # TOML test fixture parsing
rayon = "1.8"            # Parallel processing
```

## Testing

### Data-Driven Parity Tests (TOML Fixtures)

Test cases live in `tests/fixtures/{department}/{cop_name}.toml`. There is one TOML file per RuboCop cop (606 total). These are **extracted via RSpec monkey-patching** from RuboCop's actual test suite — all string interpolation, `let` blocks, and shared contexts are fully resolved.

Run tests:
```bash
cargo test --test tester
```

Check fixture statistics:
```bash
cargo run --bin fixture_stats
```

### How Test Extraction Works

Scripts live in `.claude/skills/rubocop-test-importer/scripts/`:

1. **`download_rubocop_specs.sh`** — Clones the full RuboCop repo to `/tmp/rubocop-repo` and runs `bundle install`
2. **`test_data_capture.rb`** — Module prepended onto `RuboCop::RSpec::ExpectOffense` that intercepts `expect_offense`, `expect_no_offenses`, and `expect_correction` to capture fully-resolved test data
3. **`extract_via_rspec.rb`** — Runs RSpec specs programmatically, collects captured data, generates TOML

To re-sync all fixtures:
```bash
/rubocop-test-importer sync
```

Or manually:
```bash
# 1. Clone RuboCop and install dependencies
.claude/skills/rubocop-test-importer/scripts/download_rubocop_specs.sh

# 2. Extract all tests
cd /tmp/rubocop-repo && /opt/homebrew/opt/ruby/bin/bundle exec /opt/homebrew/opt/ruby/bin/ruby \
  /Users/naveenraj/sources/devtools/ruby-fast-cop/.claude/skills/rubocop-test-importer/scripts/extract_via_rspec.rb \
  --output /Users/naveenraj/sources/devtools/ruby-fast-cop/tests/fixtures

# Extract a single cop:
cd /tmp/rubocop-repo && bundle exec ruby extract_via_rspec.rb --output <fixtures_dir> --cop Style/RaiseArgs

# Extract a department:
cd /tmp/rubocop-repo && bundle exec ruby extract_via_rspec.rb --output <fixtures_dir> --department lint
```

### TOML Fixture Format

```toml
cop = "Style/RaiseArgs"
department = "style"
severity = "convention"
implemented = true          # Set to true when cop is implemented in Rust

[[tests]]
name = "test_name_here"
source = '''
raise RuntimeError, 'message'
'''
corrected = '''              # Optional: expected autocorrect output
raise RuntimeError.new('message')
'''
base_indent = 2              # Optional: indent to restore before running

[[tests.offenses]]           # Empty `offenses = []` for no-offense tests
line = 1
column_start = 0
column_end = 30
message = "Provide an exception class and message as arguments to `raise`."

[tests.config]               # Optional: cop-specific config overrides
EnforcedStyle = "exploded"
```

### How `tester.rs` Works

1. Discovers all `tests/fixtures/**/*.toml` files
2. Skips cops with `implemented = false`
3. For each implemented cop's tests, builds a `Config` from `[tests.config]`, decodes source (restoring `base_indent`), runs the cop, and compares offenses
4. Reports mismatches in offense count, line, column, or message


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

## Common Tasks

### Adding a new cop

1. Read the TOML fixture: `tests/fixtures/{department}/{cop_name}.toml`
2. Spot-check a few test cases against the original RuboCop spec if anything looks off:
   ```bash
   curl -s "https://raw.githubusercontent.com/rubocop/rubocop/master/spec/rubocop/cop/{department}/{cop_name}_spec.rb"
   ```
3. Create file in `src/cops/{department}/{cop_name}.rs`
4. Implement `Cop` trait
5. Add to department's `mod.rs`
6. Register in cop registry
7. Set `implemented = true` in the TOML fixture
8. Run `cargo test --test tester` — verify tests pass
9. If tests fail unexpectedly, compare with original RuboCop spec and fix implementation or TOML
10. Update README.md (implemented cops table), COPS.md (status column + summary counts), and CLAUDE.md (cop count)

### Fixing a partial cop

If any implemented cops have test failures after changes, fix them:

1. Run `cargo test --test tester 2>&1 | grep "Failures in.*{cop_name}"` to see failing tests
2. Read the failing test cases in the TOML fixture
3. Compare with the original RuboCop spec to understand the expected behavior
4. Fix the Rust implementation to handle the missing cases
5. Run `cargo test --test tester` to verify

### Re-syncing test fixtures

When RuboCop releases a new version:

1. Update the version in `download_rubocop_specs.sh`
2. Run `/rubocop-test-importer sync`
3. Run `cargo test --test tester` to check for regressions
4. Update README.md with new cop/test counts if changed

### Adding a new formatter
1. Create file in `src/formatters/{name}.rs`
2. Implement `Formatter` trait
3. Register in formatter factory
4. Add CLI flag support

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

## Library API

This crate is both a CLI binary and a library. The library API allows embedding in other tools like `ruby-fast-lsp`.

Key design principles:
- Keep public API minimal and stable
- Expose functions to check source code directly (not just files)
- Make core types serializable (serde) for easy integration
- Avoid exposing internal types (AST nodes, parser details)

The CLI (`main.rs`) should be a thin wrapper around the library (`lib.rs`).

## Performance Targets

- Parse 1000 files: < 1 second
- Lint 1000 files (common cops): < 2 seconds
- Should be 50-100x faster than RuboCop

## Environment Notes

- Ruby (for test extraction only): `/opt/homebrew/opt/ruby/bin/ruby` (installed via Homebrew)
- RuboCop clone location: `/tmp/rubocop-repo`
- RuboCop version tracked: v1.85.0

## References

- [RuboCop docs](https://docs.rubocop.org/rubocop/)
- [Prism Ruby parser](https://github.com/ruby/prism)
- [ruby-prism crate](https://crates.io/crates/ruby-prism)
- [Ruff (inspiration)](https://github.com/astral-sh/ruff)
