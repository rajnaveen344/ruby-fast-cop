---
name: rubocop-test-importer
description: Import and validate RuboCop test fixtures. Use for syncing tests from RuboCop specs, checking coverage, fixing invalid YAML files, or manually importing specific cops.
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
| `sync`           | Download specs and sync all tests to YAML  |
| `download`       | Only download RuboCop specs (no sync)      |
| `validate`       | Run validation to check for issues         |
| `fix <dept/cop>` | Manually fix a specific invalid YAML file  |

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
- `sync_rubocop_tests.rb` - Extracts tests from RSpec to YAML fixtures
- `validate_test_coverage.rb` - Validates all YAML files and reports issues
- `show_spec_for_manual_sync.rb` - Shows spec content for manual YAML creation

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
3. Create a valid YAML file at `tests/fixtures/<dept>/<cop>.yaml`
4. Include `# NOTE: This file was manually synced` comment
5. Handle edge cases:
   - Replace literal tabs with spaces or use quoted strings with `\t`
   - Ensure consistent indentation in source blocks
   - Escape special YAML characters properly

## YAML Format

```yaml
cop: Department/CopName
department: department
severity: convention # or warning, error, fatal
implemented: false # set true when cop is implemented in Rust

# NOTE: This file was manually synced from RuboCop specs.

tests:
  - name: descriptive_test_name
    source: |
      ruby_code_here
    offenses:
      - line: 1
        column_start: 1
        column_end: 10
        message: "Error message"
    corrected: | # Optional
      corrected_code
    config: # Optional
      EnforcedStyle: something
    ruby_version: ">= 3.1" # Optional
```

## Implemented Cops

These cops have `implemented: true` in their YAML:

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
