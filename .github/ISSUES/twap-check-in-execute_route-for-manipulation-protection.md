# TWAP check in execute_route for manipulation protection

**Labels:** smart-contract, enhancement, security, help wanted

## Context

The PriceOracle (`contracts/price_oracle/src/lib.rs`) implements a 1-hour TWAP oracle. Reporters submit price samples via `submit_price`, and `get_twap(base_token, quote_token)` (line 154) returns the time-weighted average. `MaxDeviation` is stored at init (line 59) but never enforced — no function reads it.

Meanwhile `execute_route` (line 311) executes swaps without price sanity checks. An attacker manipulating a low-liquidity pool price could trick `get_best_route` into returning a favorable quote, then execute before the price normalizes. The 60-second quote validity window (line 40) is the attack timing target.

The PriceOracle doc comment (line 9) states it is "Used by RouteAggregator to validate that swap quotes are within acceptable bounds" — this integration doesn't exist yet.

## What done looks like

- RouteAggregator stores an optional `oracle: Address` reference to PriceOracle, set during init or via admin setter.
- Before executing each leg, the aggregator calls `PriceOracle::get_twap(base_token, quote_token)` to get the TWAP reference price.
- Compares the quote's implied price (`expected_out / amount_in`) against the TWAP. If absolute deviation exceeds `MaxDeviation` in bps, reverts with `PriceDeviationExceeded`.
- Uses TWAP not latest price so single-block spikes don't bypass it.
- If no oracle configured or no samples for the pair, check is skipped (graceful degradation).
- Tests: execution succeeds within TWAP bounds, reverts when deviation exceeds threshold, succeeds when no oracle set.

**Relevant files:**
- `contracts/price_oracle/src/lib.rs` — `get_twap` (line 154), `MaxDeviation` (line 59), `get_price_data` (line 176)
- `contracts/route_aggregator/src/lib.rs` — `execute_route` (line 311), `get_best_route` (line 210)
