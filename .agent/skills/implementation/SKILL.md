---
name: Implementation Workflow
description: TDD workflow for implementing features
---

# Skill: TDD Implementation

## When to use this skill

- Implementing new features
- Major refactoring
- Complex bug fixes

## Available templates

| Template | Usage |
|----------|-------|
| `templates/test_module.md` | Test module structure |
| `templates/module_structure.md` | DDD structure for new module |

## TDD Workflow (Test-Driven Development)

### Phase 1: Understand

1. **Read existing documentation**
   - `GLOBAL_APP_DESCRIPTION.md` for global context
   - `docs/STRATEGIES.md` if related to trading
   
2. **Identify affected modules**
   - Which DDD layer? (domain/application/infrastructure)
   - Which existing files will be impacted?

3. **Verify architecture**
   ```
   domain/         → Pure business logic, no I/O
   application/    → Orchestration, services, use cases
   infrastructure/ → I/O, external APIs, persistence
   ```

### Phase 2: Write tests FIRST

**Fundamental rule**: Tests are written BEFORE implementation code.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_feature_basic_case() {
        // Arrange
        let input = ...;
        
        // Act
        let result = new_feature(input);
        
        // Assert
        assert_eq!(result, expected);
    }

    #[test]
    fn test_new_feature_edge_case() {
        // Test edge cases
    }

    #[test]
    fn test_new_feature_error_case() {
        // Test error cases
    }
}
```

**Tests MUST fail** at this stage (red phase).

### Phase 3: Implement

1. **Minimal code** to make tests pass
2. **Respect DDD architecture**
3. **No `.unwrap()`** in production code
4. **Use `Decimal`** for amounts

```rust
// ❌ Avoid
fn process(value: f64) -> f64 {
    some_operation(value).unwrap()
}

// ✅ Prefer
fn process(value: Decimal) -> Result<Decimal, ProcessError> {
    some_operation(value).map_err(ProcessError::from)
}
```

### Phase 4: Refactor

1. **Clean the code**
   - Remove duplicate code
   - Remove commented-out code and obsolete comments
   - Improve variable names
   - Simplify complex expressions

2. **Add documentation**
   - Rustdoc on public functions
   - Comments for complex logic

3. **Verify with clippy**
   ```bash
   cargo clippy --all-targets -- -D warnings
   ```

### Phase 5: Validate

1. **Invoke the `testing` skill**
   - cargo fmt
   - cargo clippy
   - cargo test

2. **Invoke the `documentation` skill**
   - Update GLOBAL_APP_DESCRIPTION.md
   - Add entry in GLOBAL_APP_DESCRIPTION_VERSIONS.md
   - Increment version

## Complete example

```rust
// 1. First the test
#[test]
fn test_calculate_position_size() {
    let capital = Decimal::from(10000);
    let risk_pct = Decimal::from_str("0.02").unwrap();
    let stop_loss = Decimal::from_str("0.05").unwrap();
    
    let size = calculate_position_size(capital, risk_pct, stop_loss);
    
    assert_eq!(size, Decimal::from(4000)); // 2% risk, 5% stop = 40% position
}

// 2. Then the implementation
pub fn calculate_position_size(
    capital: Decimal,
    risk_percentage: Decimal,
    stop_loss_percentage: Decimal,
) -> Decimal {
    let risk_amount = capital * risk_percentage;
    risk_amount / stop_loss_percentage
}
```

## Anti-patterns to avoid

| Anti-pattern | Problem | Solution |
|--------------|---------|----------|
| Tests after code | Confirmation bias | Strict TDD |
| `.unwrap()` everywhere | Panics in production | Error handling |
| `f64` for money | Rounding errors | `Decimal` |
| Tests without assertions | False positives | At least one assertion per test |
| Modify tests to pass | Hides bugs | Fix the code |
| Commented-out code | Clutters codebase | Remove it (use git history) |
