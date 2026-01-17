# Architecture Specifications

## Core Principals

1. **Domain-Driven Design (DDD)**
   - **Domain**: Pure logic, no external dependencies.
   - **Application**: Use cases, orchestration.
   - **Infrastructure**: Implementation details (DB, API, UI).

2. **Agentic System**
   - **Autonomy**: Agents run in their own loops.
   - **Communication**: Async channels (Tokio mpsc/broadcast).
   - **Facade**: `SystemClient` abstracts agent communication.

## Communication Matrix

| Sender | Receiver | Message Type | Purpose |
|--------|----------|--------------|---------|
| Sentinel | Analyst | `MarketUpdate` | Price data |
| Analyst | RiskManager | `TradeProposal` | New entry signal |
| RiskManager | Executor | `OrderRequest` | Validated order |
| Executor | RiskManager | `OrderFill` | Position update |
| User | System | `Command` | User action |

## Data Flow Rules

1. **Market Data**: Websocket -> Sentinel -> Broadcast Channel -> (Analyst, UI)
2. **Trading**: Strategy -> Signal -> Proposal -> Risk Check -> Order -> Execution
3. **Persistance**:
   - **Hot State**: In-memory (Channels/Actors)
   - **Cold State**: SQLite (Settings, History)

## Technical Constraints

- **Concurrency**: Tokio async runtime.
- **Serialization**: Serde for all DTOs.
- **Precision**: `rust_decimal::Decimal` for ALL financial data.
- **Error Handling**: `anyhow` for app, `thiserror` for lib.
