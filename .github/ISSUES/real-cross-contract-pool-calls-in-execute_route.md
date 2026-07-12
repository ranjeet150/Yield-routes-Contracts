# Real cross-contract pool calls in execute_route

**Labels:** smart-contract, enhancement, help wanted

## Context

`execute_route()` in `contracts/route_aggregator/src/lib.rs` currently works against a single pool. It pulls input tokens and collects the protocol fee, then sets `actual_out = quote.expected_out` without calling any pool contract (lines 343-346):

```rust
// In production: for each leg, call pool.swap_a_for_b(amount_in, min_leg_out)
// For now, simulate the output
let actual_out = quote.expected_out;
```

The aggregator can never execute a real swap — it's a simulation stub.

The `RouteLeg` struct (line 67) already carries pool address, token pair, amount, and expected output. `RouteQuote` stores a `Vec<RouteLeg>`. The plumbing for multi-hop routing is structurally in place — it just needs the real cross-contract invocation.

Lines 115-122 explain the design intent: registered pools must implement `swap_a_for_b`, `swap_b_for_a`, `quote_a_for_b`, `quote_b_for_a`, and the aggregator should use `env.invoke_contract(&pool, &Symbol::new(&env, "swap_a_for_b"), args)` rather than a generated client, since the pool's contract type isn't known at compile time.

## What done looks like

- `execute_route` iterates `quote.legs` and calls `env.invoke_contract(&leg.pool, &Symbol::new(&env, "swap_a_for_b"), args)` for each leg, passing `amount_in` and a per-leg min output derived from `min_out`.
- Output tokens from each leg are held by the aggregator and fed into the next leg. Final leg output goes to the sender.
- Intermediate transfers happen within the same transaction via contract custody.
- Slippage guard compares each leg's actual output against `expected_out` and reverts if final output < `min_out`.
- Tests: single-leg execution against mock pool, two-leg multi-hop, slippage revert.

**Relevant files:**
- `contracts/route_aggregator/src/lib.rs` — `execute_route` (line 311), `RouteLeg` (line 67), cross-contract note (line 115)
