# Template: Test Module

Use this template when creating tests for a new module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    // ============================================================
    // Test Fixtures
    // ============================================================

    fn create_test_fixture() -> TestType {
        // Create reusable test data
        TestType::default()
    }

    // ============================================================
    // Happy Path Tests
    // ============================================================

    #[test]
    fn test_feature_basic_case() {
        // Arrange
        let input = create_test_fixture();

        // Act
        let result = function_under_test(input);

        // Assert
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), expected_value);
    }

    #[test]
    fn test_feature_with_valid_params() {
        // Arrange
        let param1 = dec!(100);
        let param2 = dec!(0.05);

        // Act
        let result = calculate_something(param1, param2);

        // Assert
        assert_eq!(result, dec!(5));
    }

    // ============================================================
    // Edge Cases
    // ============================================================

    #[test]
    fn test_feature_with_zero_input() {
        let result = function_under_test(dec!(0));
        assert_eq!(result, dec!(0));
    }

    #[test]
    fn test_feature_with_negative_input() {
        let result = function_under_test(dec!(-100));
        // Assert expected behavior for negative input
    }

    #[test]
    fn test_feature_with_large_values() {
        let result = function_under_test(dec!(1_000_000_000));
        // Assert no overflow or precision issues
    }

    // ============================================================
    // Error Cases
    // ============================================================

    #[test]
    fn test_feature_returns_error_on_invalid_input() {
        let result = function_under_test(invalid_input);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ExpectedError::InvalidInput));
    }

    #[test]
    fn test_feature_handles_missing_data() {
        let result = function_under_test(None);
        assert!(result.is_err());
    }

    // ============================================================
    // Integration-like Tests
    // ============================================================

    #[test]
    fn test_full_workflow() {
        // Test a complete workflow from start to finish
        let input = create_test_fixture();
        
        let step1_result = step1(input);
        assert!(step1_result.is_ok());
        
        let step2_result = step2(step1_result.unwrap());
        assert!(step2_result.is_ok());
        
        let final_result = step3(step2_result.unwrap());
        assert_eq!(final_result, expected_final_value);
    }
}
```

## Async Test Template

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tokio;

    #[tokio::test]
    async fn test_async_function() {
        // Arrange
        let client = create_mock_client();

        // Act
        let result = async_function(&client).await;

        // Assert
        assert!(result.is_ok());
    }
}
```

## Test Naming Convention

| Type | Pattern | Example |
|------|---------|---------|
| Happy path | `test_{feature}_{scenario}` | `test_calculate_position_with_valid_params` |
| Edge case | `test_{feature}_with_{edge}` | `test_calculate_position_with_zero_capital` |
| Error | `test_{feature}_returns_error_on_{condition}` | `test_calculate_position_returns_error_on_negative_risk` |
