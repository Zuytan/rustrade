# Observability Setup

Rustrade exposes Prometheus metrics on the server API (`cargo run --bin server`) at
`GET /metrics`. The endpoint uses the same port and protection as the REST API:

- Port: `PORT` or `API_PORT` (default `8080`).
- Bound to `127.0.0.1` unless `API_BIND_ADDRESS` is set.
- When `API_KEY` is set, the `X-API-Key` header is required (also for `/metrics`).

Independently, with `OBSERVABILITY_ENABLED=true`, the server prints a `METRICS_JSON:`
snapshot to stdout every `OBSERVABILITY_INTERVAL` seconds (default 60).
`OBSERVABILITY_PORT` is not used.

## 1. Configure Prometheus

```bash
prometheus --config.file=monitoring/prometheus.yml
```

`monitoring/prometheus.yml` scrapes `http://localhost:8080/metrics` and loads the alert
rules in `monitoring/alerts.yml`. If `API_KEY` is set, uncomment the `http_headers`
block (Prometheus >= 2.54) and point it to a file containing the key.

## 2. Configure Grafana

1. Start Grafana and log in (`http://localhost:3000`).
2. Add a Prometheus data source pointing to your Prometheus server
   (default `http://localhost:9090`).
3. Import `monitoring/grafana_dashboard.json` ("Rustrade Operations") and select that data source.

## 3. Available Metrics

Reliability (use these for alerting):

| Metric | Meaning |
|--------|---------|
| `rustrade_agent_up{agent}` | 1 when the agent reports Healthy |
| `rustrade_agent_last_heartbeat{agent}` | Unix time of the agent's last heartbeat |
| `rustrade_connection_up{component}` | 1 when the `market_data` / `execution` connection is online (refreshed at scrape time) |
| `rustrade_executor_reconciled` | 1 when pending orders are reconciled with the broker; 0 means new buys are refused |
| `rustrade_orders_unknown_outcome` | Orders whose submission outcome at the broker is still unknown |
| `rustrade_circuit_breaker_status` | 1 when the circuit breaker has tripped |

Portfolio and activity: `rustrade_portfolio_cash_usd`, `rustrade_portfolio_value_usd`,
`rustrade_positions_count`, `rustrade_uptime_seconds`, `rustrade_api_latency_seconds`,
`rustrade_websocket_reconnects_total`, `rustrade_trade_signals_total`, `rustrade_orders_total`.

`rustrade_portfolio_value_usd` is updated by the stdout reporter (cost basis, not market
value) and stays at 0 when `OBSERVABILITY_ENABLED` is off.

## 4. Alerting Rules

`monitoring/alerts.yml` defines:

| Alert | Condition | Severity |
|-------|-----------|----------|
| `RustradeDown` | `/metrics` unreachable for 1m | critical |
| `RustradeAgentStale` | no agent heartbeat for 60s | critical |
| `RustradeAgentNotHealthy` | agent not Healthy for 2m | warning |
| `RustradeBrokerConnectionDown` | broker connection offline for 2m | critical |
| `RustradeOrdersNotReconciled` | executor not reconciled for 2m | critical |
| `RustradeUnknownOrderOutcome` | unknown order outcome for 2m | warning |
| `RustradeCircuitBreakerTripped` | circuit breaker tripped | critical |

Route them with Alertmanager (or Grafana alerting) to your notification channel.
