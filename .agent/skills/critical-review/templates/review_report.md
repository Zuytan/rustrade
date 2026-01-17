# Template: Critical Review Report

Use this template after implementing a feature to document your self-review:

---

## Review Report: {Feature/Module Name}

**Date**: YYYY-MM-DD  
**Reviewer**: Self-review  
**Files Modified**: 
- `path/to/file1.rs`
- `path/to/file2.rs`

---

### Summary

Brief description of what was implemented (2-3 sentences).

---

### Self-Evaluation

| Criterion | Score (1-5) | Notes |
|-----------|-------------|-------|
| Readability | ? | |
| Robustness | ? | |
| Testability | ? | |
| Maintainability | ? | |
| Performance | ? | |

**Average score**: ?/5

---

### Positive Points

- ✅ Strength 1
- ✅ Strength 2
- ✅ Strength 3

---

### Points to Improve

| Priority | Issue | Impact | Action |
|----------|-------|--------|--------|
| P0 | Critical issue | Blocking | Required action |
| P1 | Important issue | Significant | To be planned |
| P2 | Improvement | Minor | Desirable |

---

### Quality Checklist

- [ ] No `.unwrap()` in production
- [ ] `Decimal` used for amounts
- [ ] Tests present and relevant
- [ ] Documentation up to date
- [ ] Clippy without warnings
- [ ] No duplicate code
- [ ] Explicit names

---

### Technical Debt Identified

| Item | Description | Estimated effort |
|------|-------------|------------------|
| ? | ? | Low/Medium/High |

---

### Decision

- [ ] ✅ Ready for commit
- [ ] ⚠️ Needs minor corrections
- [ ] ❌ Needs significant refactoring

---

### Additional Notes

Observations, open questions, or points to monitor.
