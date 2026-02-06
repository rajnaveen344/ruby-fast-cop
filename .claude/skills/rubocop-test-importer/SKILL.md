---
name: rubocop-test-importer
description: Import and validate RuboCop test fixtures. Use for syncing tests from RuboCop specs, checking coverage, fixing invalid TOML files, or manually importing specific cops.
argument-hint: "[status|sync|download|validate|fix <dept/cop>]"
allowed-tools: Bash(ruby *), Bash(chmod *), Bash(./scripts/*), Bash(find *), Bash(grep *), Bash(wc *), Bash(cd *), Bash(bundle *), Read, Write, Glob
hooks:
  post:
    - command: "ruby .claude/skills/rubocop-test-importer/scripts/validate_test_coverage.rb 2>/dev/null | head -20 || true"
      run_on_failure: true
---

# RuboCop Test Importer

Import and manage test fixtures extracted from RuboCop's RSpec test suite.

## How It Works

Tests are extracted by running the actual RSpec specs with monkey-patched `expect_offense` / `expect_no_offenses` / `expect_correction` methods. By the time these methods are called, RSpec has evaluated all `let` blocks, shared contexts, and string interpolation, so we get fully-resolved test data without any placeholders.

## Commands

Use `/rubocop-test-importer <command>`:

| Command          | Description                                |
| ---------------- | ------------------------------------------ |
| `status`         | Show current sync status and invalid files |
| `sync`           | Download repo, install deps, extract tests |
| `download`       | Only download RuboCop repo (no extraction) |
| `validate`       | Run validation to check for issues         |
| `fix <dept/cop>` | Manually fix a specific invalid TOML file  |

## Quick Start

```bash
# Check current status
/rubocop-test-importer status

# Full sync (download + bundle install + extract + validate)
/rubocop-test-importer sync

# Fix a specific invalid file
/rubocop-test-importer fix layout/line_length
```

## Scripts Location

All scripts are in `.claude/skills/rubocop-test-importer/scripts/`:

- `download_rubocop_specs.sh` - Full clone of RuboCop repo + `bundle install`
- `extract_via_rspec.rb` - Runs RSpec specs with monkey-patching to capture test data
- `test_data_capture.rb` - Monkey-patch module for `expect_offense` et al.
- `validate_test_coverage.rb` - Validates all TOML files and reports issues

## Handling Commands

### For `status` command:

Run the validation script and show summary:

```bash
ruby .claude/skills/rubocop-test-importer/scripts/validate_test_coverage.rb
```

### For `sync` command:

1. Download and setup RuboCop repo:
   ```bash
   .claude/skills/rubocop-test-importer/scripts/download_rubocop_specs.sh
   ```
2. Extract tests via RSpec:
   ```bash
   cd /tmp/rubocop-repo && bundle exec ruby <project>/.claude/skills/rubocop-test-importer/scripts/extract_via_rspec.rb --output <project>/tests/fixtures
   ```
3. Validation runs automatically via post-hook

Options for `extract_via_rspec.rb`:
- `--output DIR` — output fixtures directory (required)
- `--rubocop-dir DIR` — RuboCop repo path (default: `/tmp/rubocop-repo`)
- `--department DEPT` — process only one department
- `--cop COP` — process only one cop (e.g., `Style/RaiseArgs`)

### For `download` command:

```bash
.claude/skills/rubocop-test-importer/scripts/download_rubocop_specs.sh
```

### For `validate` command:

```bash
ruby .claude/skills/rubocop-test-importer/scripts/validate_test_coverage.rb
```

### For `fix <dept/cop>` command:

1. Read the existing TOML file at `tests/fixtures/<dept>/<cop>.toml`
2. Fetch the original RuboCop spec for reference
3. Fix the TOML file manually
4. Use `'''` literal strings for source/corrected (no escaping needed)
5. Handle edge cases:
   - If source contains `'''`, use basic strings with escaping instead
   - Use `base_indent = N` if source has decreasing indentation

## TOML Format

```toml
cop = "Department/CopName"
department = "department"
severity = "convention"  # or warning, error, fatal
implemented = false      # set true when cop is implemented in Rust

[[tests]]
name = "descriptive_test_name"
source = '''
ruby_code_here
'''
offenses = []

[[tests]]
name = "test_with_offense"
source = '''
bad_code_here
'''

[[tests.offenses]]
line = 1
column_start = 0
column_end = 10
message = "Error message"

[tests.config]
EnforcedStyle = "something"
```

### Key format rules:
- `source`/`corrected` use multi-line literal strings (`'''`) — no escaping needed
- Empty offenses: `offenses = []` (inline)
- Non-empty offenses: `[[tests.offenses]]` (array of tables)
- Config: `[tests.config]` sub-table (when present)
- Optional fields (`corrected`, `config`, `ruby_version`) omitted when not needed
- `base_indent = N` for source with indentation that was stripped

## Implemented Cops

These cops have `implemented = true` in their TOML:

- Lint/Debugger
- Lint/AssignmentInCondition
- Layout/LineLength
- Metrics/BlockLength
- Style/AutoResourceCleanup
- Style/FormatStringToken
- Style/HashSyntax
- Style/MethodCalledOnDoEndBlock
- Style/RaiseArgs
- Style/RescueStandardError
- Style/StringMethods
