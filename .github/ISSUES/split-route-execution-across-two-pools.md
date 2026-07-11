# Split route execution across two pools

**Complexity**: high

## Summary
Large orders suffer high price impact on a single path. Add split routing: `get_best_route()` can return a `RouteType::Split` where 60% of the order goes through one pool and 40% through another.

## Acceptance criteria
- [ ] `RouteLeg` extended with `split_pct: u32` field
- [ ] `execute_route()` handles Split type: two sub-executions in same tx
- [ ] `get_best_route()` compares split vs single-path output and returns best
- [ ] Unit test: split route returns better price than single route for large amount

## Stellar Wave
Points: 200 | Complexity: 🔴 High
