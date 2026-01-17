---
description: Complete workflow for implementing a feature
---

# Workflow: Feature Implementation

## Steps

1. **Read the context**
   - Consult `GLOBAL_APP_DESCRIPTION.md` to understand the system
   - Identify affected modules

2. **Consult the appropriate skill**
   - Read `.agent/skills/implementation/SKILL.md`
   - If trading feature: also read `.agent/skills/rust-trading/SKILL.md`

3. **Write tests first (TDD)**
   - Unit tests for the logic
   - Integration tests if needed
   - Tests must fail initially

4. **Implement the feature**
   - Minimal code to make tests pass
   - Respect DDD architecture
   - Use `Decimal` for amounts

5. **Validate the code**
   ```bash
   cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
   ```

6. **Update documentation**
   - Consult `.agent/skills/documentation/SKILL.md`
   - Update `GLOBAL_APP_DESCRIPTION.md`
   - Add entry in `GLOBAL_APP_DESCRIPTION_VERSIONS.md`
   - Increment version in `Cargo.toml`

7. **Update skills if necessary**
   - If new strategy/indicator → update `.agent/skills/rust-trading/SKILL.md`
   - If new test workflow → update `.agent/skills/testing/SKILL.md`
   - If new benchmark command → update `.agent/skills/benchmarking/SKILL.md`
   - If new implementation pattern → update `.agent/skills/implementation/SKILL.md`

8. **Critical review**
   - Consult `.agent/skills/critical-review/SKILL.md`
   - Use `templates/quick_checklist.md` to validate

9. **Commit**
   - Descriptive commit message
   - Reference issue if applicable
