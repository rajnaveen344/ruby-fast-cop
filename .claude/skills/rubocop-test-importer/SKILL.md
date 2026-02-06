---
name: rubocop-test-importer
description: Import and validate RuboCop test fixtures. Use for syncing tests from RuboCop specs, checking coverage, fixing invalid TOML files, or manually importing specific cops.
argument-hint: "[status|sync|download|validate|fix <dept/cop>]"
allowed-tools: Bash(ruby *), Bash(chmod *), Bash(./scripts/*), Bash(find *), Bash(grep *), Bash(wc *), Read, Write, Glob
hooks:
  post:
    - command: "ruby .claude/skills/rubocop-test-importer/scripts/validate_test_coverage.rb 2>/dev/null | head -20 || true"
      run_on_failure: true
---

# RuboCop Test Importer

Import and manage test fixtures extracted from RuboCop's RSpec test suite.

## Commands

Use `/rubocop-test-importer <command>`:

| Command          | Description                                |
| ---------------- | ------------------------------------------ |
| `status`         | Show current sync status and invalid files |
| `sync`           | Download specs and sync all tests to TOML  |
| `download`       | Only download RuboCop specs (no sync)      |
| `validate`       | Run validation to check for issues         |
| `fix <dept/cop>` | Manually fix a specific invalid TOML file  |

## Quick Start

```bash
# Check current status
/rubocop-test-importer status

# Full sync (download + extract + validate)
/rubocop-test-importer sync

# Fix a specific invalid file
/rubocop-test-importer fix layout/line_length
```

## Scripts Location

All scripts are in `.claude/skills/rubocop-test-importer/scripts/`:

- `download_rubocop_specs.sh` - Downloads RuboCop specs via sparse git checkout
- `sync_rubocop_tests.rb` - Extracts tests from RSpec to TOML fixtures
- `validate_test_coverage.rb` - Validates all TOML files and reports issues
- `show_spec_for_manual_sync.rb` - Shows spec content for manual TOML creation

## Handling Commands

### For `status` command:

Run the validation script and show summary:

```bash
ruby .claude/skills/rubocop-test-importer/scripts/validate_test_coverage.rb
```

### For `sync` command:

1. Download specs: `.claude/skills/rubocop-test-importer/scripts/download_rubocop_specs.sh`
2. Run sync: `ruby .claude/skills/rubocop-test-importer/scripts/sync_rubocop_tests.rb`
3. Validation runs automatically via post-hook

### For `download` command:

```bash
.claude/skills/rubocop-test-importer/scripts/download_rubocop_specs.sh
```

### For `validate` command:

```bash
ruby .claude/skills/rubocop-test-importer/scripts/validate_test_coverage.rb
```

### For `fix <dept/cop>` command:

1. Run: `ruby .claude/skills/rubocop-test-importer/scripts/show_spec_for_manual_sync.rb <dept/cop>`
2. Read the RSpec file shown
3. Create a valid TOML file at `tests/fixtures/<dept>/<cop>.toml`
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
- Optional fields (`corrected`, `config`, `ruby_version`, `interpolated`, `verified`) omitted when not needed
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
