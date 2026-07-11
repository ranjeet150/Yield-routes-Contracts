# YieldVault — Contract Structure Audit

## File: `src/lib.rs`

### What it does
SEP-56 Tokenized Vault Standard implementation. Accepts USDC deposits, issues
vault shares, auto-compounds LP fees and DEX rebates every 6 hours via an
off-chain harvest bot, and allows share redemption at any time.

### Structure overview
- `DataKey` enum — storage keys (Admin, Asset, TotalShares, Paused, etc.)
- SEP-56 interface: `deposit`, `redeem`, `withdraw`, `mint`, `convert_to_shares`,
  `convert_to_assets`, `total_assets`, `query_asset`
- SEP-41 token interface on vault shares (transfer, approve, allowance, etc.)
- `harvest(gross_yield)` — callable by authorised bots only

### Strengths
- Full SEP-56 compliance (deposit/redeem/withdraw/mint all implemented)
- Share price calculated from `total_assets / total_supply` — manipulation-resistant
- `pause/unpause` admin functions for emergency stops
- Performance fee applied inside the contract, not trust-passed from off-chain

### Issues / Improvements needed
1. **Harvest authorisation is a single key** — the `harvest_bot` address can drain
   yield. Replace with a multi-sig or require a Stellar threshold signature.

2. **No deposit cap** — large single deposits can manipulate the share price for
   subsequent depositors. Add `max_deposit_assets` and `max_deposit_shares` per SEP-56.

3. **Rounding direction** — when converting assets→shares on deposit, round *down*
   (favours the vault). When converting shares→assets on redeem, round *down* too
   (favours the vault, not the redeemer). Verify `Decimal` usage matches this.

4. **No `max_redeem` / `max_withdraw` views** — SEP-56 requires these to report
   the maximum a user can actually redeem/withdraw given liquidity constraints.

5. **`total_assets()` reads on-chain balance directly** — if the vault's asset
   balance can be manipulated by a flash loan, share price is momentarily inflated.
   Use a snapshot/TWAP of assets instead.

6. **Missing events** — add `Deposit`, `Withdraw`, `Harvest` events with indexed
   `depositor` field so the indexer can reconstruct deposit history trustlessly.

### Recommended additions
- `set_performance_fee_bps(bps: u32)` capped at 3000 (30%)
- `set_harvest_bot(address: Address)` with old-bot revocation
- `preview_mint(shares) -> assets` and `preview_withdraw(assets) -> shares` views
- Emergency `sweep_token(token, recipient)` for admin (non-vault-asset only)
