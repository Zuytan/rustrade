# Template: Version Entry

Use this template when adding a new version to `GLOBAL_APP_DESCRIPTION_VERSIONS.md`:

```markdown
## Version X.Y.Z - YYYY-MM-DD

### Added
- New feature description

### Changed
- Modified behavior description

### Fixed
- Bug fix description

### Removed
- Removed feature description (if any)

### Technical
- Internal changes (refactoring, dependencies, etc.)
```

## Rules

1. **Date format**: YYYY-MM-DD (ISO 8601)
2. **Version**: Follow SemVer (MAJOR.MINOR.PATCH)
3. **Categories**: Only include sections that have content
4. **Style**: Start each item with a verb (Added, Changed, Fixed, Removed)

## Common Verbs

| Action | Verb |
|--------|------|
| New feature | Added |
| Modified behavior | Changed, Updated, Improved |
| Bug correction | Fixed, Corrected |
| Deletion | Removed, Deleted |
| Performance | Optimized |
| Refactoring | Refactored, Reorganized |

## Example

```markdown
## Version 0.8.5 - 2026-01-17

### Added
- Agentic development files (AGENTS.md, skills, workflows)
- Benchmarking skill with performance metrics documentation

### Changed
- Restructured `.agent/` directory with modular skills

### Technical
- Added 6 new skills for AI agent guidance
```
