# Contributing to Rustrade

Thank you for your interest in contributing to Rustrade! This document provides guidelines and instructions for contributing.

## 🚀 Getting Started

### Prerequisites

- **Rust** (latest stable, 2024 edition)
- **SQLite** (for local database)
- **Alpaca API Keys** (for live/paper trading) - [Get them here](https://app.alpaca.markets/)

### Development Setup

```bash
# Clone the repository
git clone https://github.com/zuytan/rustrade.git
cd rustrade

# Copy environment template
cp .env.example .env
# Edit .env with your API keys (paper trading recommended)

# Build the project
cargo build

# Run tests
cargo test

# Run the application
cargo run --bin rustrade
```

## 📋 Code Style

### Formatting & Linting

All code must pass formatting and linting checks before merging:

```bash
# Format code
cargo fmt

# Run clippy (must pass with no warnings)
cargo clippy -- -D warnings

# Run all tests
cargo test
```

### Rust Guidelines

- **No `.unwrap()` in production code** - Use proper error handling with `?` or `anyhow`
- **Document public APIs** - Use `///` doc comments for public functions and structs
- **Write tests** - New features should include unit tests
- **Keep functions small** - Prefer functions under 50 lines

### Architecture

Rustrade follows **Domain-Driven Design (DDD)**:

```
src/
├── domain/        # Core business logic, entities, value objects
├── application/   # Use cases, services, orchestration
├── infrastructure/# External adapters (API, DB, UI)
└── interfaces/    # UI components, CLI handlers
```

When adding features:
1. **Domain first** - Define entities and business rules
2. **Application layer** - Create services that orchestrate domain objects
3. **Infrastructure** - Add adapters for external systems
4. **Tests** - Write tests at each layer

## 🔀 Pull Request Process

### Before Submitting

1. **Create an issue** describing the feature/bug (optional for small fixes)
2. **Fork the repository** and create a feature branch
3. **Write tests** for new functionality
4. Ensure all checks pass:
   ```bash
   cargo fmt --check
   cargo clippy -- -D warnings
   cargo test
   ```
5. **For trading strategies/risk management**: Run the trading code review script:
   ```bash
   ./scripts/review_trading_code.sh
   ```

### PR Guidelines

- **Keep PRs focused** - One feature or fix per PR
- **Write clear descriptions** - Explain what and why
- **Reference issues** - Use `Fixes #123` or `Relates to #456`
- **Update documentation** - If adding user-facing features

### Trading Code Review Requirements ⚠️

**If your PR modifies trading strategies, risk management, or financial calculations**, it must pass additional strict review requirements:

- ✅ Use `rust_decimal::Decimal` for ALL monetary calculations (NO f64/f32)
- ✅ Implement dynamic risk-based position sizing (NO hardcoded quantities)
- ✅ Define strict stop losses for all trade signals
- ✅ Strategies must only return `Signal`, NOT execute orders directly
- ✅ Include comprehensive unit and integration tests

See **[REVIEW_GUIDELINES.md](REVIEW_GUIDELINES.md)** for complete requirements.

Your PR will be automatically checked by the `trading-review.yml` GitHub Action.

### Commit Messages

Follow conventional commits:

```
feat: add VWAP strategy implementation
fix: correct RSI calculation for edge cases
docs: update strategy documentation
refactor: extract order validation to separate module
test: add unit tests for risk manager
```

## 🧪 Testing

### Running Tests

```bash
# All tests
cargo test

# Specific module
cargo test risk_management

# With output
cargo test -- --nocapture

# Doc tests only
cargo test --doc
```

### Test Guidelines

- **Unit tests** in the same file as the code (`#[cfg(test)]` module)
- **Integration tests** in the `tests/` directory
- **Mock external services** - Don't make real API calls in tests

## 📚 Documentation

### Code Documentation

Use Rust doc comments for public APIs:

```rust
/// Calculates position size based on risk parameters.
///
/// # Arguments
/// * `capital` - Total available capital
/// * `risk_pct` - Maximum risk per trade (0.01 = 1%)
///
/// # Returns
/// The recommended position size in units
pub fn calculate_position_size(capital: Decimal, risk_pct: f64) -> Decimal {
    // ...
}
```

### Strategy Documentation

When adding a new trading strategy:
1. Add implementation in `src/application/strategies/`
2. Update `docs/STRATEGIES.md` with algorithm explanation
3. Add the strategy to `StrategyMode` enum and factory

## 🐛 Reporting Bugs

When reporting bugs, please include:

1. **Environment** - OS, Rust version, app version
2. **Steps to reproduce**
3. **Expected vs actual behavior**
4. **Relevant logs** (with sensitive data redacted)

## 💡 Feature Requests

We welcome feature ideas! Please:

1. Check existing issues first
2. Describe the use case
3. Propose a solution (optional)

## ⚠️ Important Notes

> **WARNING**: This project is for educational purposes. Never use with real money
> without thorough testing and understanding of the risks involved.

## 📄 License

By contributing, you agree that your contributions will be licensed under the MIT License.

---

Thank you for contributing to Rustrade! 🦀📈
