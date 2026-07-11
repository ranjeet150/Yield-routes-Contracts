# RouteAggregator — Contract Structure Audit

## File: `src/lib.rs`

### What it does
Finds the optimal swap path across registered Soroban AMM pool contracts.
Supports up to 6-hop chained swaps and maintains a registry of approved pools.

### Structure overview
- `DataKey` enum — storage keys (Admin, Pool, Quote, etc.)
- `PoolInfo` / `RouteLeg` / `RouteQuote` structs — core data types
- `RouteAggregatorTrait` — public contract interface
- `RouteAggregator` struct + `#[contract]` impl — entry points

### Strengths
- Admin-gated pool registry (`register_pool`, `deregister_pool`)
- Quote expiry enforced on-chain (60s TTL)
- Price impact guard (`max_impact_bps`)
- Per-quote unique ID prevents replay

### Issues / Improvements needed
1. **No reentrancy guard** — `execute_route` calls external pool contracts in a loop.
   Add a `Executing` lock key in storage, set before the loop, clear after.

2. **Fee not enforced on-chain** — `protocol_fee` is computed off-chain and passed in.
   Compute it inside the contract from a stored `fee_bps` value instead.

3. **Slippage check is per-leg, not end-to-end** — a user could receive less than
   `min_out` overall if intermediate legs just pass the per-leg threshold.
   Add a final check: `actual_out >= min_out` after all legs execute.

4. **Admin is a single address** — use a 2-of-3 multisig or timelock for upgrades
   in production to prevent a single key compromise from draining pools.

5. **Events** — emit a `SwapExecuted { sender, token_in, token_out, amount_in, amount_out }`
   event so the off-chain indexer can reconstruct volume stats without trusting the API.

6. **Pool validity** — `register_pool` should verify the pool address is a deployed
   contract (call `pool.get_reserves()`) before adding it to the registry.

### Recommended additions
- `pause() / unpause()` admin functions for emergency stops
- `set_fee_bps(bps: u32)` with a cap (e.g. 100 bps max)
- `get_registered_pools() -> Vec<PoolInfo>` read function for the frontend
