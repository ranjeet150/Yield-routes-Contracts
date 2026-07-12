# Split route execution across two pools

**Labels:** smart-contract, enhancement, help wanted

## Context

A single swap might need to be split across multiple pools when no single pool has enough liquidity. For example, routing 6,000 USDC → XLM through Soroswap and 4,000 through Aquarius AMM may yield a better effective price than sending the full amount through one pool.

The RouteAggregator already has the pool registry infrastructure:

- `register_pool(token_a, token_b, pool)` — stores a pool keyed by the token pair, both orderings (line 156).
- `get_pool_list()` — returns all registered pairs as `Vec<(Address, Address)>` (line 426).
- `get_pool(token_a, token_b)` — looks up a single pool (line 197).

`RouteType` (line 60) has `DirectAmm` but no split variant. Line 18 documents the intent: "Split (future): divide order across two pools for better price impact."

The challenge is that `get_best_route` currently only considers one pool per pair. To support splits, it needs to discover multiple pools for a token pair, query each for quoted output, and determine whether a proportional split yields better aggregate output.

## What done looks like

- New `RouteType::SplitAmm` variant added to the enum.
- `get_best_route` checks for multiple pools per pair, queries each via `quote_a_for_b`, and computes the optimal split ratio maximizing total output.
- A split function divides `amount_in` proportionally (weighted by each pool's quoted depth) and returns combined `RouteLeg` entries.
- `execute_route` handles `SplitAmm` by executing each leg and aggregating outputs.
- `price_impact_bps` reflects blended impact across split legs.
- Tests: split yields more output than single-pool, fallback to single-pool when one pool registered.

**Relevant files:**
- `contracts/route_aggregator/src/lib.rs` — `register_pool` (line 156), `get_pool_list` (line 426), `RouteType` (line 60), split note (line 18)
