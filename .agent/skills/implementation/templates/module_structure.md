# Template: New Module Structure

Use this template when creating a new module following DDD:

## Domain Layer (`src/domain/{module_name}/`)

```
src/domain/{module_name}/
├── mod.rs              # Module re-exports
├── {entity}.rs         # Main entity
├── {value_object}.rs   # Value objects (if needed)
└── {service}.rs        # Domain service (pure logic)
```

### mod.rs
```rust
//! {Module Name} domain module.
//!
//! This module contains the core business logic for {description}.

mod {entity};
mod {value_object};

pub use {entity}::*;
pub use {value_object}::*;
```

### Entity Template
```rust
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// {Entity description}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct {EntityName} {
    pub id: String,
    pub field1: Decimal,
    pub field2: String,
}

impl {EntityName} {
    /// Create a new {EntityName}
    pub fn new(id: String, field1: Decimal, field2: String) -> Self {
        Self { id, field1, field2 }
    }

    /// {Method description}
    pub fn calculate_something(&self) -> Decimal {
        // Pure business logic, no I/O
        self.field1 * Decimal::from(2)
    }
}
```

---

## Application Layer (`src/application/{module_name}/`)

```
src/application/{module_name}/
├── mod.rs              # Module re-exports
└── {service}.rs        # Application service
```

### Service Template
```rust
use crate::domain::{module}::{Entity};
use anyhow::Result;

/// Application service for {description}
pub struct {ServiceName} {
    // Dependencies injected here
}

impl {ServiceName} {
    pub fn new() -> Self {
        Self {}
    }

    /// {Method description}
    pub async fn do_something(&self, entity: &Entity) -> Result<()> {
        // Orchestration logic
        // Call domain methods
        // Coordinate with infrastructure
        Ok(())
    }
}
```

---

## Infrastructure Layer (`src/infrastructure/{module_name}/`)

```
src/infrastructure/{module_name}/
├── mod.rs              # Module re-exports
└── {repository}.rs     # External I/O
```

### Repository Template
```rust
use crate::domain::{module}::{Entity};
use anyhow::Result;

/// Repository for persisting/fetching {Entity}
pub struct {RepositoryName} {
    // Database connection, API client, etc.
}

impl {RepositoryName} {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn save(&self, entity: &Entity) -> Result<()> {
        // Persistence logic
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<Entity>> {
        // Fetch logic
        Ok(None)
    }
}
```

---

## Checklist

- [ ] Domain entity created with pure business logic
- [ ] No I/O in domain layer
- [ ] Application service for orchestration
- [ ] Infrastructure for external interactions
- [ ] Tests for each layer
- [ ] Module registered in parent `mod.rs`
