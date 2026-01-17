# Checklist: Quick Critical Review

Use this checklist for a quick self-review after any implementation:

## Code Quality

- [ ] Functions are under 50 lines
- [ ] No complex nested conditionals (max 3 levels)
- [ ] Variable names are self-explanatory
- [ ] No magic numbers (use constants)
- [ ] No commented-out code

## Error Handling

- [ ] No `.unwrap()` in production code
- [ ] Error messages are informative
- [ ] All Result types are handled
- [ ] Proper error propagation with `?`

## Trading-Specific

- [ ] `Decimal` for all money calculations
- [ ] No `f64` for prices, quantities, amounts
- [ ] Risk checks are in place
- [ ] Edge cases handled (zero, negative, overflow)

## Tests

- [ ] Tests exist for new code
- [ ] Tests cover happy path
- [ ] Tests cover edge cases
- [ ] Tests cover error cases
- [ ] All tests pass

## Architecture

- [ ] Respects DDD layers (domain/app/infra)
- [ ] No domain -> infrastructure dependency
- [ ] Single Responsibility Principle followed
- [ ] Low coupling between modules

## Performance (if applicable)

- [ ] No unnecessary allocations in loops
- [ ] No blocking calls in async code
- [ ] Appropriate data structures used

## Documentation

- [ ] Public functions have rustdoc
- [ ] Complex logic has comments
- [ ] GLOBAL_APP_DESCRIPTION.md updated (if needed)

## Skills & Agents (if new feature)

- [ ] Skills in `.agent/skills/` updated if new pattern
- [ ] Templates updated if new workflow
- [ ] Scripts updated if new command

---

**Result**: [ ] PASS  [ ] NEEDS WORK

**Notes**: 
