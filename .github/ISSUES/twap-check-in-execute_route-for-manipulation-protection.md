# TWAP check in execute_route for manipulation protection

**Complexity**: high

## Summary
`execute_route()` should validate that the swap price is within `max_deviation_bps` of the TWAP oracle price before execution.

## Acceptance criteria
- [ ] `execute_route()` invokes `PriceOracle.get_twap(token_in, token_out)` cross-contract
- [ ] If `|actual_rate - twap_rate| > max_deviation_bps` → panic with new error `PriceDeviationTooHigh`
- [ ] `max_deviation_bps` stored in `RouteAggregator` storage, admin-updateable
- [ ] Unit test: deviation within bounds passes, exceeds fails

## Stellar Wave
Points: 200 | Complexity: 🔴 High
