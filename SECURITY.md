# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.94.x  | :white_check_mark: |
| < 0.94  | :x:                |

## Reporting a Vulnerability

Please report vulnerabilities via email to `security@rustrade.bot`. Do not open public issues for sensitive security flaws.

## Best Practices

### API Keys
- Never commit `.env` files.
- Use `chmod 600 .env` to restrict read permissions.
- Rotate API keys every 90 days.
- Use "Trade Only" API permissions (disable Withdrawal).

### Data Privacy
- Trading history is stored locally in `rustrade.db` (SQLite).
- Ensure your disk is encrypted (FileVault/BitLocker).

### Dependency Management
- We use `cargo-audit` in CI to detect vulnerabilities.
- Dependencies are pinned in `Cargo.lock`.
