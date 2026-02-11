---
name: Testing & Validation
description: Test and validation workflow before commit
---

# Skill: Testing & Validation

## When to use this skill

- **BEFORE any commit** (mandatory)
- After implementing a feature
- To validate a refactoring
- When in doubt about code quality

## Automated script

```bash
# Full validation (format + clippy + test)
./.agent/skills/testing/scripts/validate.sh

# With auto-fix formatting
./.agent/skills/testing/scripts/validate.sh --fix
```

## Validation workflow

### Step 1: Format

```bash
cargo fmt --all
```

Automatically formats all code according to Rust conventions.

### Step 2: Lint

```bash
cargo clippy --all-targets -- -D warnings
```

**Rules:**
- NO warnings allowed
- Fix clippy suggestions, don't ignore them with `#[allow(...)]`
- If an allow is really necessary, justify it in a comment

### Step 3: Tests

```bash
cargo test
```

**Rules:**
- All tests must pass
- A failing test = no commit
- New tests should cover edge cases

### Async Tests (MANDATORY)

When writing `#[tokio::test]`, preventing infinite hangs is critical:

1. **Always use timeouts** for channel operations (`recv().await`).
   ```rust
   // ❌ Potential hang
   let msg = rx.recv().await.unwrap();

   // ✅ Safe
   let msg = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
       .await
       .expect("Timeout waiting for message")?;
   ```
2. **Avoid infinite loops** without exit conditions or timeouts.

### Step 4: Release verification (optional)

```bash
cargo build --release
```

Do this before a release or to verify optimizations.

## Quick commands

```bash
# Full validation in one line
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test

# Specific tests
cargo test test_name           # A specific test
cargo test module_name::       # All tests in a module
cargo test --lib               # Library tests only
cargo test --test integration  # Integration tests
```

## On failure

### Clippy warning
1. Read the error message carefully
2. Apply clippy's suggestion
3. If the suggestion is not applicable, justify with a comment

### Test failure
1. Identify the failing test
2. Check if it's a bug in the code or in the test
3. NEVER delete a test to make CI pass
4. Fix the code or adapt the test if behavior changed intentionally

## Checklist before commit

- [ ] `cargo fmt` executed
- [ ] `cargo clippy` without warnings
- [ ] `cargo test` all tests pass
- [ ] Documentation updated (if needed)
- [ ] GLOBAL_APP_DESCRIPTION.md updated (if new feature)
- [ ] Version incremented in Cargo.toml
