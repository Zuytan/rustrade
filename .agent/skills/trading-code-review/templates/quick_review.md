# Quick Trading Code Review Checklist

**PR**: [Link to PR]  
**Reviewer**: [Your Name]  
**Date**: [Date]

---

## ⛔ CRITICAL BLOCKERS (Zero Tolerance)

- [ ] **NO f64/f32 for money**: All prices, quantities, P&L use `Decimal`
- [ ] **NO direct order execution**: Strategies only return `Signal`
- [ ] **Stop losses defined**: All signals use `.with_stop_loss()`
- [ ] **Dynamic position sizing**: No hardcoded quantities

**Blockers Found**: ____  

If > 0: ⛔ **CANNOT MERGE** - Request changes immediately

---

## 🟡 QUANTITATIVE QUALITY

- [ ] Parameter count reasonable (<5)
- [ ] No look-ahead bias
- [ ] Transaction costs modeled
- [ ] Uses `ta` crate for indicators
- [ ] No obvious overfitting

**Warnings Found**: ____

---

## ✅ CODE QUALITY

- [ ] Tests included (unit + integration)
- [ ] Edge cases handled
- [ ] No `.unwrap()` in production
- [ ] Passes clippy
- [ ] Documentation added

---

## VERDICT

- [ ] **APPROVE** - All checks passed
- [ ] **REQUEST CHANGES** - Blockers present
- [ ] **COMMENT** - Warnings only

---

**Notes**:

[Add specific feedback here]
