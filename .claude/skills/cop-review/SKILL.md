---
name: cop-review
description: "Review changed code for reuse, quality, and efficiency, then fix any issues found. Post-implementation review that compares Rust cop against original RuboCop Ruby source to evaluate complexity, identify simplification opportunities, and ensure structural parity."
---

# Cop Implementation Review

Run this after implementing a cop and all tests pass. The goal is to compare our Rust implementation against RuboCop's Ruby source and evaluate whether we can simplify.

## Steps

### 1. Identify which cops were changed

Look at recent git changes to find new or modified cop files:
```bash
git diff --name-only HEAD~1  # or appropriate range
```

### 2. For each cop, fetch the original RuboCop source

```
https://raw.githubusercontent.com/rubocop/rubocop/master/lib/rubocop/cop/{department}/{cop_name}.rb
```

If the cop uses mixins, also fetch:
```
https://raw.githubusercontent.com/rubocop/rubocop/master/lib/rubocop/cop/mixin/{mixin_name}.rb
```

### 3. Compare and evaluate

For each cop, assess these dimensions:

**Size ratio**: Count lines (excluding blank lines and comments) in both versions.
- Target: Rust should be ~1.5-2.5x the Ruby line count (Rust is more verbose by nature)
- Red flag: >3x means we're likely over-engineering

**Structural parity**: Does our Rust code follow the same logical flow as the Ruby?
- Same conditions checked in the same order?
- Same edge cases handled?
- If Ruby uses a simple iteration, are we using a simple iteration (not a complex visitor)?

**Unnecessary complexity**: Look for:
- Deeply nested match arms that could be flattened
- Helper functions called only once (inline them)
- Redundant intermediate data structures
- Over-abstracted traits/enums for simple concepts
- Excessive `.clone()` or `.to_string()` that could be avoided with borrows

**Missing reuse**: Check if:
- Logic duplicated with another cop should move to `src/helpers/`
- Existing helpers in `source.rs`, `escape.rs`, or other helper files could replace hand-written code
- A new shared helper would benefit multiple cops

**Dead code**: Look for:
- Config options parsed but never used
- Match arms that can never trigger given the visitor pattern
- Defensive checks that Prism's type system already guarantees

### 4. Report findings

Summarize findings per cop in this format:

```
## {CopName}
- **Ruby**: ~X lines | **Rust**: ~Y lines | **Ratio**: Z:1
- **Verdict**: [Good / Simplify / Refactor]
- **Issues** (if any):
  - [specific issue and suggested fix]
```

### 5. Apply fixes

If issues are found:
- Apply simplifications directly
- Run `cargo test --test tester` to verify nothing breaks
- If extracting shared helpers, ensure all cops using them still pass

## Guidelines

- Don't force the Rust code to look like Ruby — idiomatic Rust patterns are fine even if they differ structurally
- A flat `match` with many arms is fine if each arm is simple — don't abstract it into a trait just to reduce arms
- Prefer readability over cleverness — a slightly longer but clear implementation beats a compact but cryptic one
- If the Ruby cop is itself complex (>200 lines), the Rust being 3x is acceptable
- Don't add comments just to match Ruby's comments — only comment non-obvious logic
