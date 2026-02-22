# Trading Code Review System - Documentation Index

Complete guide to the Rustrade trading code review system.

## 📖 Documentation Map

### For Contributors (Start Here!)

1. **[Quick Reference Card](QUICK_REFERENCE.md)** ⚡
   - One-page cheat sheet
   - Critical rules at a glance
   - Common patterns
   - **Read first** (5 minutes)

2. **[How-To Guide](REVIEW_HOWTO.md)** 📝
   - Step-by-step instructions
   - For contributors and reviewers
   - Common scenarios
   - Troubleshooting
   - **Essential reading** (15 minutes)

3. **[Violation Examples](REVIEW_EXAMPLES.md)** 💡
   - 6 complete examples
   - Violations and corrections
   - Expected feedback
   - **Study before coding** (20 minutes)

### For Reviewers

4. **[Review Guidelines](../REVIEW_GUIDELINES.md)** 📋
   - Complete reference manual
   - All rules with examples
   - Review checklist
   - Output templates
   - **Complete reference** (45 minutes)

5. **[Review Templates](.agent/skills/trading-code-review/templates/)** 📄
   - Quick review checklist
   - Detailed review report
   - **Use for consistency**

### For Maintainers

6. **[System Overview](TRADING_CODE_REVIEW.md)** 🏗️
   - Architecture and components
   - File structure
   - Integration points
   - **System documentation** (20 minutes)

7. **[Implementation Summary](REVIEW_SYSTEM_SUMMARY.md)** 📊
   - What was built
   - How it works
   - Maintenance guide
   - **Technical reference** (15 minutes)

## 🎯 Quick Navigation

### By Task

| I want to... | Read this... |
|--------------|--------------|
| Submit a trading PR | [Quick Reference](QUICK_REFERENCE.md) → [How-To Guide](REVIEW_HOWTO.md) |
| Review a trading PR | [Review Guidelines](../REVIEW_GUIDELINES.md) → [Templates](.agent/skills/trading-code-review/templates/) |
| Understand violations | [Examples](REVIEW_EXAMPLES.md) |
| Learn the system | [System Overview](TRADING_CODE_REVIEW.md) |
| Maintain the system | [Implementation Summary](REVIEW_SYSTEM_SUMMARY.md) |

### By Role

| Role | Primary Docs | Secondary Docs |
|------|--------------|----------------|
| **New Contributor** | Quick Reference, How-To Guide | Examples |
| **Experienced Contributor** | Quick Reference | Guidelines (reference) |
| **Reviewer** | Review Guidelines, Templates | Examples |
| **Maintainer** | System Overview, Implementation Summary | All docs |

### By Time Available

| Time | Start Here |
|------|-----------|
| **5 minutes** | [Quick Reference](QUICK_REFERENCE.md) |
| **15 minutes** | [How-To Guide](REVIEW_HOWTO.md) |
| **30 minutes** | [Examples](REVIEW_EXAMPLES.md) + [How-To](REVIEW_HOWTO.md) |
| **1 hour** | [Guidelines](../REVIEW_GUIDELINES.md) |
| **Full deep dive** | All docs in order |

## 📁 File Structure

```
rustrade/
├── REVIEW_GUIDELINES.md          # Complete reference manual
├── docs/
│   ├── QUICK_REFERENCE.md        # One-page cheat sheet  
│   ├── REVIEW_HOWTO.md           # Step-by-step guide
│   ├── REVIEW_EXAMPLES.md        # Violation examples
│   ├── TRADING_CODE_REVIEW.md    # System overview
│   ├── REVIEW_SYSTEM_SUMMARY.md  # Implementation details
│   └── INDEX.md                  # This file
├── scripts/
│   └── review_trading_code.sh    # Automated review script
├── .github/
│   ├── workflows/
│   │   └── trading-review.yml    # GitHub Actions workflow
│   ├── pull_request_template.md  # Default PR template
│   └── PULL_REQUEST_TEMPLATE/
│       └── trading_strategy.md   # Trading PR template
├── .cargo/
│   └── clippy.toml               # Custom lint rules
└── .agent/skills/
    └── trading-code-review/
        ├── SKILL.md              # Agent skill guide
        └── templates/            # Review templates
```

## 🚀 Getting Started

### First Time Using the System

1. **Read** [Quick Reference](QUICK_REFERENCE.md) (5 min)
2. **Read** [How-To Guide](REVIEW_HOWTO.md) (15 min)
3. **Study** [Examples](REVIEW_EXAMPLES.md) (20 min)
4. **Practice** Fix an example violation (30 min)
5. **Ready!** Start your contribution

### Before Every PR

1. Run `./scripts/review_trading_code.sh`
2. Check [Quick Reference](QUICK_REFERENCE.md) for rules
3. Use trading PR template
4. Wait for automated checks

### Performing a Review

1. Run `./scripts/review_trading_code.sh`
2. Follow [Review Guidelines](../REVIEW_GUIDELINES.md)
3. Use [Templates](.agent/skills/trading-code-review/templates/)
4. Provide clear feedback

## 🎓 Learning Path

### Beginner
- [ ] Read Quick Reference
- [ ] Read How-To Guide (contributor section)
- [ ] Study Examples (sections 1-3)
- [ ] Fix one example violation
- [ ] Run review script
- [ ] Submit first trading PR

### Intermediate
- [ ] Read complete Guidelines
- [ ] Study all Examples
- [ ] Understand quantitative concerns
- [ ] Review architectural patterns
- [ ] Perform code reviews

### Advanced
- [ ] Master all guidelines
- [ ] Understand system architecture
- [ ] Contribute to review system
- [ ] Mentor new contributors
- [ ] Maintain documentation

## 🔍 Search Guide

Looking for specific information?

### Rules and Requirements
- **Float types**: [Guidelines](../REVIEW_GUIDELINES.md#1-monetary-precision-mandatory-blocker)
- **Stop losses**: [Guidelines](../REVIEW_GUIDELINES.md#21-stop-losses-mandatory)
- **Position sizing**: [Guidelines](../REVIEW_GUIDELINES.md#22-position-sizing-mandatory)
- **Architecture**: [Guidelines](../REVIEW_GUIDELINES.md#4-architectural-compliance)

### Code Examples
- **Decimal usage**: [Examples](REVIEW_EXAMPLES.md#example-1-float-type-violation)
- **Stop loss**: [Examples](REVIEW_EXAMPLES.md#example-2-missing-stop-loss)
- **Position sizing**: [Examples](REVIEW_EXAMPLES.md#example-3-hardcoded-position-size)
- **All patterns**: [Quick Reference](QUICK_REFERENCE.md#common-patterns)

### Processes
- **Submitting PR**: [How-To](REVIEW_HOWTO.md#for-contributors)
- **Reviewing PR**: [How-To](REVIEW_HOWTO.md#for-reviewers)
- **Running script**: [How-To](REVIEW_HOWTO.md#4-run-the-review-script)
- **Fixing issues**: [How-To](REVIEW_HOWTO.md#5-fix-any-issues)

### Tools
- **Review script**: `scripts/review_trading_code.sh`
- **GitHub Action**: `.github/workflows/trading-review.yml`
- **Clippy config**: `.cargo/clippy.toml`
- **Templates**: `.agent/skills/trading-code-review/templates/`

## 📊 Document Statistics

| Document | Lines | Words | Purpose |
|----------|-------|-------|---------|
| REVIEW_GUIDELINES.md | 499 | ~15,000 | Complete reference |
| REVIEW_HOWTO.md | 442 | ~10,000 | Usage guide |
| REVIEW_EXAMPLES.md | 340 | ~9,000 | Examples |
| TRADING_CODE_REVIEW.md | 260 | ~7,000 | System overview |
| REVIEW_SYSTEM_SUMMARY.md | 235 | ~7,000 | Implementation |
| QUICK_REFERENCE.md | 134 | ~3,500 | Cheat sheet |
| **Total** | **1,910** | **~51,500** | Full documentation |

## 🛠️ Tools Reference

| Tool | Location | Usage |
|------|----------|-------|
| Review Script | `scripts/review_trading_code.sh` | `./scripts/review_trading_code.sh [path]` |
| GitHub Action | `.github/workflows/trading-review.yml` | Runs automatically on PR |
| Clippy Config | `.cargo/clippy.toml` | `cargo clippy --all-targets -- -D warnings` |
| Quick Review | `.agent/skills/trading-code-review/templates/quick_review.md` | Copy and fill out |
| Detailed Review | `.agent/skills/trading-code-review/templates/detailed_review.md` | Copy and fill out |

## 🔗 External Resources

- [Contributing Guide](../CONTRIBUTING.md)
- [Agent Skills](../.agent/skills/)
- [Project README](../README.md)
- [Architecture Overview](../GLOBAL_APP_DESCRIPTION.md)

## 📞 Support

- **Questions**: Open issue with `question` label
- **Bug reports**: Open issue with `review-system` label  
- **Documentation improvements**: Submit PR
- **Unclear requirements**: Ask in PR comments

## 🎉 Quick Wins

Want to contribute immediately?

1. **Improve docs**: Fix typos, add examples
2. **Enhance script**: Add new checks
3. **Create examples**: Document more patterns
4. **Test system**: Try to find edge cases
5. **Help others**: Answer questions in issues

## 📝 Maintenance

For maintainers updating the system:

1. **Update rules**: Edit REVIEW_GUIDELINES.md
2. **Add checks**: Modify scripts/review_trading_code.sh
3. **New examples**: Add to REVIEW_EXAMPLES.md
4. **Improve docs**: Update any of these files
5. **Version**: Update REVIEW_SYSTEM_SUMMARY.md

## ✅ Success Checklist

You're ready when you can:

- [ ] List the 4 critical blocker rules
- [ ] Explain why f64 is dangerous for money
- [ ] Write a signal with stop loss
- [ ] Calculate risk-based position size
- [ ] Run the review script
- [ ] Fix common violations
- [ ] Use the trading PR template

## 🌟 Best Practices

1. **Start simple**: Read Quick Reference first
2. **Practice**: Fix example violations
3. **Ask early**: Questions are welcome
4. **Review often**: Run script during development
5. **Be thorough**: Trading bugs cost money

---

**Welcome to the Rustrade trading code review system!**

Start with the [Quick Reference](QUICK_REFERENCE.md) and you'll be contributing in no time. 🚀
