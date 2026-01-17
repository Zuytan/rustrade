---
name: Project Specifications Management
description: Manage and enforce project specifications for consistency
---

# Skill: Spec Management

## When to use this skill

- **Before implementation**: To analyze impact (`specs/features.md`).
- **During architecture changes**: To verify boundaries (`specs/modules.md`).
- **When adding dependencies**: To check constraints (`specs/architecture.md`).
- **After major changes**: To update the specs to reflect reality.

## Available Specifications

| File | Specs Content |
|------|---------------|
| `specs/architecture.md` | Technical stack, communication patterns, data flow rules |
| `specs/features.md` | Feature list, dependencies, impact analysis matrix |
| `specs/modules.md` | Module boundaries, allowed/forbidden dependencies |
| `specs/README.md` | Index and general usage |

## Workflow: Impact Analysis

Before writing code, answer these questions using `specs/features.md`:

1. **What am I modifying?** (e.g., Risk Manager)
2. **Who depends on this?** (e.g., Executor, UI)
3. **What constraints apply?** (e.g., Decimal precision, Async)

## Workflow: Updating Specs

If your code change modifies the system behavior (new flow, new module, new dependency), you **MUST** update the specs.

### Checklist for Spec Update

- [ ] Does the architectural diagram in `specs/modules.md` need update?
- [ ] Did I add a new cross-cutting concern? Update `specs/features.md`.
- [ ] Did I change a communication pattern? Update `specs/architecture.md`.

## Integration with Implementation

The `/implement` workflow includes a check step. When in doubt:

1. **Read** `specs/features.md` to see what you might break.
2. **Implement** your change.
3. **Verify** that you respected `specs/modules.md` boundaries.
4. **Update** specs if you introduced something new.
