# Real cross-contract pool calls in execute_route

**Complexity**: high

## Summary
`RouteAggregator.execute_route()` currently simulates the swap output without calling the actual pool contract. Production implementation must invoke the registered AMM pool's swap function via cross-contract call.

## Acceptance criteria
- [ ] For each `RouteLeg`, call `pool.swap_a_for_b(amount_in, min_out)` via `invoke_contract`
- [ ] Actual output returned by pool used as `actual_out`
- [ ] Multi-leg routes: output of leg N is input to leg N+1
- [ ] Test: integration test on testnet with real AMM pool
- [ ] Slippage guard checked after all legs executed

## Stellar Wave
Points: 200 | Complexity: 🔴 High
