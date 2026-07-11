# FeeDistributor — Contract Structure Audit

## File: `src/lib.rs`

### What it does
Collects protocol fees from RouteAggregator and YieldVault and distributes
them on-chain to three recipients: treasury, the yield vault (reinvestment),
and LP reward pools.

### Structure overview
- `DataKey` enum — storage keys (Admin, Treasury, VaultAddr, LpRewards, Splits)
- `DistributionSplit` struct — `{ treasury_bps: u32, vault_bps: u32, lp_bps: u32 }`
- `distribute(token, amount)` — splits and transfers in one transaction
- `set_splits(split)` — admin-gated split config
- `set_recipients(treasury, vault, lp)` — admin-gated recipient config

### Strengths
- Single-transaction distribution prevents partial sends
- Split basis-points validated to sum to 10000 on `set_splits`
- Recipient addresses are stored on-chain, not passed as call arguments

### Issues / Improvements needed
1. **`distribute` can be called by anyone** — anyone can trigger a distribution
   of tokens held by the contract. Restrict to admin or whitelisted callers,
   or limit to tokens explicitly registered as fee tokens.

2. **No minimum distribution amount** — many small calls waste gas. Add a
   `min_distribution_amount` threshold below which calls revert.

3. **Token allowance model** — the contract pulls tokens from the caller via
   `token.transfer_from`. If the RouteAggregator doesn't pre-approve, the call
   fails silently. Document the required approval flow or switch to a push model
   where RouteAggregator calls `token.transfer` directly to FeeDistributor.

4. **LP rewards recipient is a single address** — use a separate LP reward
   distribution contract that handles per-LP share accounting rather than a
   single wallet.

5. **No events** — emit `FeesDistributed { token, total, to_treasury, to_vault, to_lp }`
   for off-chain accounting.

6. **Split update is immediate** — a compromised admin could change the split
   to 100% treasury before the next distribution. Add a timelock (e.g. 24h delay)
   on `set_splits` changes.

### Recommended additions
- `register_fee_token(token: Address)` whitelist
- `get_pending_balance(token) -> i128` view
- `set_min_distribution(amount: i128)` with admin guard
- Timelock pattern for `set_splits` and `set_recipients`
