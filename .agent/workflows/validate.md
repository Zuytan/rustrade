---
description: Validate code before commit
---
// turbo-all

# Workflow: Validation

Run these commands before any commit:

1. Format code
```bash
cargo fmt --all
```

2. Lint code (zero warnings)
```bash
cargo clippy --all-targets -- -D warnings
```

3. Run tests
```bash
cargo test
```

## On failure

- **fmt**: Code will be reformatted automatically
- **clippy**: Fix warnings, don't ignore them
- **test**: Identify and fix the failing test

## Quick command

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
```
