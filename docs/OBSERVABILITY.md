# Observability Setup

## Prerequisites

- Prometheus installed (`brew install prometheus` or via Docker)
- Grafana installed (`brew install grafana` or via Docker)
- Rustrade running with `OBSERVABILITY_ENABLED=true` (or default feature `ui`).

## 1. Configure Prometheus

Use the provided `monitoring/prometheus.yml` configuration:

```bash
prometheus --config.file=monitoring/prometheus.yml
```

This will scrape Rustrade metrics from `http://localhost:9090/metrics`.

## 2. Configure Grafana

1. Start Grafana (`grafana-server`).
2. Login to `http://localhost:3000` (default admin/admin).
3. **Add Data Source**:
   - Type: Prometheus
   - URL: `http://localhost:9090`
   - Save & Test.
4. **Import Dashboard**:
   - Go to Dashboards > New > Import.
   - Upload `monitoring/grafana_dashboard.json`.
   - Select the Prometheus data source you just created.

## 3. Available Metrics

The dashboard visualizes:

- **Win Rate**: `rustrade_win_rate` (Gauge)
- **P&L**: `rustrade_pnl_total` (Gauge)
- **Drawdown**: `rustrade_drawdown_pct` (Gauge)
- **Trade Volume**: `rustrade_trades_total` (Counter)

## 4. Alerting Rules (Recommended)

Configure these alerts in Grafana:

1. **High Drawdown**
   - Condition: `rustrade_drawdown_pct > 0.05` (5%)
   - Severity: Critical

2. **Circuit Breaker Trip**
   - Condition: `circuit_breaker_status == 1`
   - Severity: Critical

3. **API Latency**
   - Condition: `rate(http_request_duration_seconds_sum[5m]) / rate(http_request_duration_seconds_count[5m]) > 0.5`
   - Severity: Warning
