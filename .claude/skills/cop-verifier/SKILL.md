---
name: cop-verifier
description: Verify interpolated RuboCop test fixtures. Use to list unverified tests, verify specific cops by resolving Ruby string interpolation and $UNRESOLVED config values.
argument-hint: "[list|stats|verify <dept/cop>|verify-test <dept/cop> <test-name>]"
allowed-tools: Bash(cargo run --bin fixture_stats *), Bash(curl *), Read, Edit, Glob, Grep, WebFetch
---

# Cop Verifier

Verify and fix interpolated test fixtures that were extracted from RuboCop's RSpec test suite. Tests marked `interpolated = true` contain unresolved Ruby string interpolation (`#{...}`) or config values (`$UNRESOLVED:`) that need manual verification.

## Commands

Use `/cop-verifier <command>`:

| Command                              | Description                                          |
| ------------------------------------ | ---------------------------------------------------- |
| `list`                               | List all unverified tests (interpolated + !verified) |
| `stats`                              | Show verification statistics by department           |
| `verify <dept/cop>`                  | Verify all interpolated tests for a specific cop     |
| `verify-test <dept/cop> <test-name>` | Verify a single test case                            |

## Quick Start

```bash
# Show statistics
/cop-verifier stats

# List all unverified tests
/cop-verifier list

# Verify a specific cop
/cop-verifier verify style/string_literals

# Verify a single test
/cop-verifier verify-test lint/debugger test_name_here
```

## What "Verification" Means

Tests marked `interpolated = true` contain Ruby code/values that weren't fully resolved during extraction:

### 1. Source Code Interpolation (`#{...}`)

```toml
# BEFORE: Unresolved - what is #{method}?
source = '''
foo.#{method}
'''

# AFTER: Resolved from original spec
source = '''
foo.upcase
'''
```

### 2. Config Values (`$UNRESOLVED:variable`)

```toml
# BEFORE: What is $UNRESOLVED:enforced_style?
[tests.config]
EnforcedStyle = "$UNRESOLVED:enforced_style"

# AFTER: Found in spec's let(:enforced_style) { 'double_quotes' }
[tests.config]
EnforcedStyle = "double_quotes"
```

## Handling Commands

### For `list` command:

Search for unverified tests and display them:

```bash
# Find files with interpolated tests
grep -r "interpolated = true" tests/fixtures/ | grep -v "verified = true" | head -50

# Or use the stats tool with verbose mode
cargo run --bin fixture_stats -- --verbose
```

### For `stats` command:

```bash
cargo run --bin fixture_stats
```

### For `verify <dept/cop>` command:

1. **Read the TOML fixture:**

   ```bash
   cat tests/fixtures/{dept}/{cop}.toml
   ```

2. **Fetch the original RuboCop spec:**

   ```bash
   curl -s "https://raw.githubusercontent.com/rubocop/rubocop/master/spec/rubocop/cop/{dept}/{cop}_spec.rb"
   ```

3. **For each test with `interpolated = true` and `verified = false`:**
   - Find the corresponding `it` block in the spec
   - Look for `let(:variable)` blocks that define interpolated values
   - Resolve all `#{...}` in source/message/offenses
   - Replace all `$UNRESOLVED:var` in config with actual values

4. **Update the TOML file:**
   - Replace interpolated source with resolved values
   - Replace `$UNRESOLVED:var` config values
   - Change `verified = false` to `verified = true`
   - Keep `interpolated = true` as a marker

### For `verify-test` command:

Same as above but for a single test case.

## Verification Workflow

### Step 1: Read the TOML fixture

```toml
# tests/fixtures/style/string_literals.toml
[[tests]]
name = "some_test_with_interpolation"
source = '''
"#{string}"
'''
interpolated = true
verified = false

[[tests.offenses]]
message = "Use #{style} quotes"

[tests.config]
EnforcedStyle = "$UNRESOLVED:enforced_style"
```

### Step 2: Fetch original RuboCop spec

```ruby
# From spec/rubocop/cop/style/string_literals_spec.rb

let(:enforced_style) { 'single_quotes' }

context 'when EnforcedStyle is single_quotes' do
  let(:string) { 'hello' }
  let(:style) { 'single' }

  it 'registers offense for double quotes' do
    expect_offense(<<~RUBY)
      "hello"
      ^^^^^^^ Use single quotes.
    RUBY
  end
end
```

### Step 3: Resolve values

```toml
# AFTER verification
[[tests]]
name = "some_test_with_interpolation"
source = '''
"hello"
'''
interpolated = true
verified = true

[[tests.offenses]]
message = "Use single quotes"

[tests.config]
EnforcedStyle = "single_quotes"
```

## Common Patterns in RuboCop Specs

### `let` blocks define variables

```ruby
let(:enforced_style) { 'snake_case' }
let(:methods) { %w[foo bar] }
```

### Nested contexts inherit and override

```ruby
context 'default config' do
  let(:style) { 'compact' }  # Outer context

  context 'with option' do
    let(:style) { 'expanded' }  # Overrides for this context
  end
end
```

### `it_behaves_like` shares examples

```ruby
it_behaves_like 'accepts', 'valid_code'
# Look for shared_examples_for 'accepts' elsewhere in the file
```

## Tips

1. **Start with simpler cops** - Some cops have complex shared examples; start with straightforward ones

2. **Look for patterns** - If multiple tests use the same `let` block, resolve them all at once

3. **Check context hierarchy** - Variables are often defined in parent contexts

4. **Verify against RuboCop** - When uncertain, run the actual test through RuboCop:

   ```bash
   echo 'code_here' | bundle exec rubocop --stdin test.rb --only Dept/CopName
   ```

5. **Keep interpolated = true** - This flag marks the test as originally interpolated for tracking purposes; only change `verified` to `true`

## Example Session

```
> /cop-verifier verify style/method_call_with_args_parentheses

Reading TOML fixture...
Found 12 interpolated tests needing verification.

Fetching original spec from GitHub...

Test 1: when_enforced_style_is_require_parentheses_registers_offense
- Source has: foo.#{method}(arg)
- Config has: EnforcedStyle: $UNRESOLVED:enforced_style
- Found in spec: let(:method) { 'bar' }, let(:enforced_style) { 'require_parentheses' }
- Resolved source: foo.bar(arg)
- Resolved config: EnforcedStyle: require_parentheses
- Setting verified: true

[... continues for remaining tests ...]

Done! Verified 12 tests. Run tests with:
  cargo test --test tester -- style::method_call_with_args_parentheses
```
