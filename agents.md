# AGENTS.md

## Project Overview

Rustrade is a high-performance algorithmic trading bot written in Rust.

**Architecture**: Domain-Driven Design (DDD) with autonomous agents communicating via channels.

**Agents**: Sentinel (data), Analyst (brain), RiskManager (gatekeeper), Executor (orders), Listener (news), User (UI).

## Agent Protocol (MANDATORY)

To ensure consistency, every Agent **MUST** start a task by:
1. **Acknowledging** the active context (Agentic Mode).
2. **Citing** the Workflow and Skills being applied.

**Example Response Start:**
> "I will proceed using the `/implement` workflow and the `rust-trading` skill."

## Communication Guidelines

- **Language**: Always communicate in the language of the development environment
- **Documentation**: All code documentation MUST be in English
- **Comments**: Code comments MUST be in English
- **Commit messages**: MUST be in English

## Critical Rules (MUST FOLLOW)

### Code Quality
- **NEVER** use `f64` for money calculations → use `rust_decimal::Decimal`
- **NEVER** use `.unwrap()` in production code → use proper error handling (`?`, `match`, `.expect()` with context)
- **ALWAYS** write tests BEFORE implementation (TDD)
- **ALWAYS** update documentation after significant changes
- **ALWAYS** perform a critical self-review after implementation

### Architecture (DDD)
```
src/
├── domain/          # Pure business logic, NO I/O, NO external deps
├── application/     # Use cases, orchestration, services
└── infrastructure/  # External services, I/O, brokers, persistence
```

### Before Every Commit
1. `cargo fmt` - Format all code
2. `cargo clippy --all-targets -- -D warnings` - Zero warnings
3. `cargo test` - All tests must pass
4. `cargo check` - Verify compilation (including features)
5. Update `GLOBAL_APP_DESCRIPTION.md` if features changed
6. Update `GLOBAL_APP_DESCRIPTION_VERSIONS.md` with changelog
7. Increment version in `Cargo.toml` (SemVer)

## Skills

This project uses modular skills in `.agent/skills/`. 
**Read the relevant SKILL.md before starting specific tasks:**

| Skill | When to use |
|-------|-------------|
| `rust-trading` | Trading features, strategies, risk management |
| `testing` | Before any commit, validation workflow |
| `documentation` | After adding features, updating architecture |
| `implementation` | TDD workflow for new features |
| `critical-review` | After implementation, self-critique of work quality |
| `benchmarking` | Strategy validation, performance metrics, backtesting |
| `trading-best-practices` | Before new strategies, quarterly reviews, staying current |
| `ui-design` | Creating/modifying UI components, layouts, styling |
| `spec-management` | Impact analysis, architecture constraints, updating specs |

## Workflows

Invoke workflows from `.agent/workflows/` for common tasks:
- `/implement` - Full feature implementation workflow
- `/validate` - Quick validation before commit

## Key Documentation

| File | Purpose |
|------|---------|
| `GLOBAL_APP_DESCRIPTION.md` | Complete system overview |
| `GLOBAL_APP_DESCRIPTION_VERSIONS.md` | Version history |
| `docs/STRATEGIES.md` | Trading strategies documentation |
| `CONTRIBUTING.md` | Contribution guidelines |

## Common Commands

```bash
# Development
cargo run --bin rustrade      # Run with UI
cargo run --bin server        # Run headless (server mode)

# Validation
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test

# Benchmarking
cargo run --bin benchmark -- --help
```
