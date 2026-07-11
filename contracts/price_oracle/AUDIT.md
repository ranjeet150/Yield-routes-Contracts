# PriceOracle — Contract Structure Audit

## File: `src/lib.rs`

### What it does
On-chain TWAP oracle fed by authorised off-chain reporter bots that read
Stellar DEX data. Used by RouteAggregator to validate swap quotes and
guard against price manipulation attacks.

### Structure overview
- `DataKey` enum — storage keys (Admin, Reporter, Price, Twap, etc.)
- `PriceEntry` struct — `{ price: i128, timestamp: u64 }`
- `submit_price(reporter, base, quote, price)` — reporter-gated write
- `get_price(base, quote) -> PriceEntry` — latest spot price
- `get_twap(base, quote) -> i128` — time-weighted average
- `add_reporter / remove_reporter` — admin-gated whitelist

### Strengths
- Reporter whitelist prevents arbitrary price submissions
- TWAP window smooths out single-block manipulation
- Prices stored as `i128` (9 decimal places) — sufficient precision for Stellar

### Issues / Improvements needed
1. **Single reporter is a single point of failure** — if the bot goes offline,
   prices go stale. Require at least 2-of-N reporters to submit before a price
   is considered valid (median aggregation).

2. **No staleness check in `get_price`** — callers receive an outdated price
   silently. Add a `max_age_seconds` param or revert if
   `env.ledger().timestamp() - entry.timestamp > MAX_STALE_SECONDS`.

3. **TWAP window is hardcoded** — make it configurable via `set_twap_window(seconds)`.

4. **No price deviation guard** — a rogue reporter can submit a price 10× the
   previous value. Reject submissions where `|new - last| / last > max_deviation_bps`.

5. **Integer overflow risk** — TWAP sum accumulates `price * time_delta`.
   At 9 decimal places over a 1-hour window, the product can exceed `i128` range
   for high-value pairs. Use `u128` for intermediate accumulation.

6. **No events** — emit `PriceSubmitted { reporter, base, quote, price, timestamp }`
   so the indexer can audit reporter activity.

### Recommended additions
- `get_reporters() -> Vec<Address>` read function
- `is_stale(base, quote, max_age: u64) -> bool` convenience view
- `set_max_deviation_bps(bps: u32)` with admin guard
