# Project Specifications

This directory contains the authoritative specifications for the Rustrade project.
Any change in the code MUST be reflected here to ensure consistency and traceability.

## Structure

| File | Purpose |
|------|---------|
| `architecture.md` | Technical specifications, patterns, data flow |
| `features.md` | Functional requirements, dependencies between features |
| `modules.md` | Module boundaries and inter-dependencies (Impact Analysis) |
| `data_dictionary.md` | Data models, units, and precision rules |

## Usage

When planning a change, consulting these specs helps identify:
1. **Impact**: Which other modules might break?
2. **Constraints**: What rules (precision, perf) must be respected?
3. **Consistency**: Does this match the existing patterns?
