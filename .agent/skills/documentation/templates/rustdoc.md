# Template: Rustdoc Function Documentation

Use this template for documenting public functions:

```rust
/// Brief one-line description of what the function does.
///
/// More detailed explanation if the function is complex.
/// Can span multiple lines.
///
/// # Arguments
///
/// * `param1` - Description of the first parameter
/// * `param2` - Description of the second parameter
///
/// # Returns
///
/// Description of the return value.
///
/// # Errors
///
/// * `ErrorType1` - When this error occurs
/// * `ErrorType2` - When this other error occurs
///
/// # Panics
///
/// Describe conditions that would cause a panic (if any).
/// Ideally, there should be none in production code.
///
/// # Examples
///
/// ```
/// use crate::module::function_name;
///
/// let result = function_name(arg1, arg2)?;
/// assert_eq!(result, expected);
/// ```
pub fn function_name(param1: Type1, param2: Type2) -> Result<ReturnType, Error> {
    // Implementation
}
```

## Minimal Template (simple functions)

```rust
/// Calculates the position size based on risk parameters.
///
/// # Arguments
/// * `capital` - Available capital
/// * `risk_pct` - Risk percentage per trade
///
/// # Returns
/// The calculated position size in units.
pub fn calculate_position_size(capital: Decimal, risk_pct: Decimal) -> Decimal {
    // Implementation
}
```

## Struct Documentation

```rust
/// Represents a trading order with execution details.
///
/// # Fields
///
/// * `symbol` - The traded symbol (e.g., "AAPL")
/// * `side` - Buy or Sell
/// * `quantity` - Number of units
/// * `price` - Limit price (if applicable)
#[derive(Debug, Clone)]
pub struct Order {
    pub symbol: String,
    pub side: OrderSide,
    pub quantity: Decimal,
    pub price: Option<Decimal>,
}
```
