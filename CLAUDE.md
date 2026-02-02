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

### Data-Driven Tests (YAML Fixtures)

Test cases are stored in `tests/fixtures/{department}/{cop_name}.yaml`. These were **extracted via script** from RuboCop's spec files, not hand-written.

**IMPORTANT: Always validate YAML fixtures against the original RuboCop specs when implementing a cop.**

The extraction script may have edge cases or errors. Before trusting a YAML fixture:

1. **Fetch the original RuboCop spec file:**
   ```bash
   # The specs are cached in /tmp/rubocop-specs (from extraction script)
   # Or fetch fresh from GitHub:
   curl -s "https://raw.githubusercontent.com/rubocop/rubocop/master/spec/rubocop/cop/{department}/{cop_name}_spec.rb"

   # Example for Lint/Debugger:
   curl -s "https://raw.githubusercontent.com/rubocop/rubocop/master/spec/rubocop/cop/lint/debugger_spec.rb"
   ```

2. **Compare key test cases:**
   - Check that offense line/column positions match the `^^^` markers in the original
   - Verify the `config:` values match `let(:cop_config)` blocks
   - Ensure `expect_no_offenses` tests have `offenses: []`
   - Check `corrected:` field matches `expect_correction` blocks

3. **Watch for extraction issues:**
   - **Interpolated strings**: Tests marked `interpolated: true` contain Ruby string interpolation (`#{...}`) that was NOT correctly extracted. You MUST:
     1. Fetch the original RuboCop spec file from GitHub
     2. Find the corresponding test case in the spec
     3. Semantically translate the Ruby test to our YAML format (replace interpolated variables with their actual values)
     4. Update the YAML fixture with the correct source code
   - Shared examples (`it_behaves_like`) may not have captured all context
   - Deeply nested `context` blocks may have missed config inheritance
   - **YAML parse errors**: Some fixtures contain tab characters or special characters that break YAML parsing. The test runner skips files with parse errors if they're marked `implemented: false`, but you must fix YAML issues before marking a cop as implemented.

4. **Validation workflow when implementing a cop:**
   ```
   Before implementing:
   1. Read the YAML fixture: tests/fixtures/{dept}/{cop}.yaml
   2. Fetch original spec from RuboCop repo
   3. Spot-check 3-5 test cases match

   After implementing:
   1. Run: cargo test --test tester
   2. If tests fail unexpectedly, compare with original spec
   3. Fix YAML if extraction was wrong, or fix implementation
   ```

5. **Re-extract if needed:**
   ```bash
   # Re-extract a single cop's tests
   cargo run --bin extract-rubocop-tests -- /tmp/rubocop-specs/spec/rubocop/cop/{dept}/{cop}_spec.rb
   ```

### Test Types

- **Parity tests** (`tests/tester.rs`) - Data-driven tests from YAML fixtures
- **Unit tests** - Cop-specific edge cases in `src/cops/{dept}/{cop}.rs`
- **Integration tests** - End-to-end CLI comparison with RuboCop
- **Benchmark tests** - Performance validation

## Performance Targets

- Parse 1000 files: < 1 second
- Lint 1000 files (common cops): < 2 seconds
- Should be 50-100x faster than RuboCop

## Common Tasks

### Adding a new cop
1. **Validate test fixtures first:**
   - Read `tests/fixtures/{department}/{cop_name}.yaml`
   - Fetch original spec: `curl -s "https://raw.githubusercontent.com/rubocop/rubocop/master/spec/rubocop/cop/{department}/{cop_name}_spec.rb"`
   - Compare 3-5 test cases to verify extraction accuracy
   - Fix YAML if needed, or note discrepancies

2. Create file in `src/cops/{department}/{cop_name}.rs`
3. Implement `Cop` trait
4. Add to department's `mod.rs`
5. Register in cop registry
6. Set `implemented: true` in the YAML fixture
7. Run `cargo test --test tester` - verify tests pass
8. **Post-implementation validation:**
   - If any test fails unexpectedly, compare with original RuboCop spec
   - Run actual RuboCop on failing test source to confirm expected behavior
   - Fix implementation or YAML as needed
9. Update COPS.md

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
