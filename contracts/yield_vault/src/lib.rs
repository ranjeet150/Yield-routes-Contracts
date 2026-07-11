//! # YieldVault — YieldRoutes (SEP-56 Compliant)
//!
//! An auto-compounding yield vault implementing the Stellar **SEP-56
//! Tokenized Vault Standard** (analogous to ERC-4626 on Ethereum).
//!
//! SEP-56 requires the vault to:
//! - Implement the full SEP-41 fungible token interface (the *share* token)
//! - Provide `deposit`, `withdraw`, `redeem`, `mint` with preview functions
//! - Expose `convert_to_shares`, `convert_to_assets`, `total_assets`, `query_asset`
//! - Use virtual shares offset to prevent inflation attacks
//!
//! ## Share price mechanics
//! ```
//! share_price = (total_assets + 1) / (total_supply + 10^DECIMALS_OFFSET)
//! shares_minted = assets * (total_supply + 10^OFFSET) / (total_assets + 1)
//! ```
//! The +1 / +offset guard follows ERC-4626's virtual liquidity pattern,
//! making first-depositor inflation attacks economically infeasible.
//!
//! ## Yield accrual
//! Admin calls `harvest(yield_amount)` to add yield to `total_assets`
//! without minting new shares → every existing share becomes worth more.
//!
//! ## SEP-56 interface functions (required)
//! ```
//! query_asset()              → Address
//! total_assets()             → i128
//! convert_to_shares(assets)  → i128
//! convert_to_assets(shares)  → i128
//! max_deposit(receiver)      → i128
//! max_mint(receiver)         → i128
//! max_withdraw(owner)        → i128
//! max_redeem(owner)          → i128
//! preview_deposit(assets)    → i128
//! preview_mint(shares)       → i128
//! preview_withdraw(assets)   → i128
//! preview_redeem(shares)     → i128
//! deposit(assets, receiver, from)            → i128  (shares minted)
//! mint(shares, receiver, from)               → i128  (assets deposited)
//! withdraw(assets, receiver, owner, operator)→ i128  (shares burned)
//! redeem(shares, receiver, owner, operator)  → i128  (assets returned)
//! ```

#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype,
    Address, Env, Symbol, String, token,
    panic_with_error,
};

/// Virtual decimals offset — matches OpenZeppelin's Stellar vault default.
/// Prevents inflation attacks by ensuring the share price starts at a
/// meaningful baseline even before the first real deposit.
const DECIMALS_OFFSET: u32 = 7; // 10^7 virtual shares/assets
const OFFSET_FACTOR: i128 = 10_000_000; // 10^7

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Asset,           // underlying token address
    TotalAssets,
    TotalShares,     // total outstanding vault share supply
    Balance(Address),// share balance per holder
    Allowance(Address, Address), // spender → amount
    Name,
    Symbol,
    Decimals,
    HarvestCount,
    TotalYieldHarvested,
    Paused,
    FeeCollector,
    PerformanceFeeBps,
}

#[contracttype]
#[derive(Clone, Debug)]
pub enum VaultError {
    NotAdmin           = 1,
    ZeroAmount         = 2,
    InsufficientShares = 3,
    VaultPaused        = 4,
    InsufficientAssets = 5,
    Overflow           = 6,
    NotAuthorized      = 7,
}

impl soroban_sdk::contracterror::ContractError for VaultError {
    fn from_val(v: u32) -> Option<Self> {
        match v {
            1 => Some(Self::NotAdmin),
            2 => Some(Self::ZeroAmount),
            3 => Some(Self::InsufficientShares),
            4 => Some(Self::VaultPaused),
            5 => Some(Self::InsufficientAssets),
            6 => Some(Self::Overflow),
            7 => Some(Self::NotAuthorized),
            _ => None,
        }
    }
}

// ─── Internal conversion helpers ─────────────────────────────────────────────

fn _total_assets(env: &Env) -> i128 {
    env.storage().instance().get(&DataKey::TotalAssets).unwrap_or(0)
}

fn _total_shares(env: &Env) -> i128 {
    env.storage().instance().get(&DataKey::TotalShares).unwrap_or(0)
}

/// SEP-56 §5.1: shares = assets × (totalSupply + 10^offset) / (totalAssets + 1)
/// Rounding DOWN (in favour of vault) per the standard.
fn _assets_to_shares(assets: i128, total_assets: i128, total_shares: i128) -> i128 {
    let virtual_supply = total_shares + OFFSET_FACTOR;
    let virtual_assets = total_assets + 1;
    assets * virtual_supply / virtual_assets
}

/// SEP-56 §5.1: assets = shares × (totalAssets + 1) / (totalSupply + 10^offset)
/// Rounding DOWN (in favour of vault) per the standard.
fn _shares_to_assets(shares: i128, total_assets: i128, total_shares: i128) -> i128 {
    let virtual_supply = total_shares + OFFSET_FACTOR;
    let virtual_assets = total_assets + 1;
    shares * virtual_assets / virtual_supply
}

fn _balance_of(env: &Env, owner: &Address) -> i128 {
    env.storage().persistent().get(&DataKey::Balance(owner.clone())).unwrap_or(0)
}

fn _mint_shares(env: &Env, to: &Address, shares: i128) {
    let bal = _balance_of(env, to);
    env.storage().persistent().set(&DataKey::Balance(to.clone()), &(bal + shares));
    let ts = _total_shares(env);
    env.storage().instance().set(&DataKey::TotalShares, &(ts + shares));
}

fn _burn_shares(env: &Env, from: &Address, shares: i128) {
    let bal = _balance_of(env, from);
    if bal < shares { panic_with_error!(env, VaultError::InsufficientShares); }
    env.storage().persistent().set(&DataKey::Balance(from.clone()), &(bal - shares));
    let ts = _total_shares(env);
    env.storage().instance().set(&DataKey::TotalShares, &(ts - shares));
}

#[contract]
pub struct YieldVault;

#[contractimpl]
impl YieldVault {

    // ── Initialisation ────────────────────────────────────────────────────────

    /// Deploy vault.
    /// `asset`       — underlying SEP-41 token (e.g., USDC)
    /// `name/symbol` — share token metadata (e.g., "YieldRoutes USDC", "yrUSDC")
    /// `fee_bps`     — performance fee on harvested yield (e.g., 1000 = 10%)
    pub fn initialize(
        env: Env,
        admin: Address,
        asset: Address,
        fee_collector: Address,
        name: String,
        symbol: String,
        decimals: u32,
        performance_fee_bps: u32,
    ) {
        if env.storage().instance().has(&DataKey::Admin) { panic!("already initialized"); }
        admin.require_auth();
        assert!(performance_fee_bps <= 3_000, "fee cannot exceed 30%");
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Asset, &asset);
        env.storage().instance().set(&DataKey::FeeCollector, &fee_collector);
        env.storage().instance().set(&DataKey::Name, &name);
        env.storage().instance().set(&DataKey::Symbol, &symbol);
        env.storage().instance().set(&DataKey::Decimals, &decimals);
        env.storage().instance().set(&DataKey::PerformanceFeeBps, &performance_fee_bps);
        env.storage().instance().set(&DataKey::TotalAssets, &0_i128);
        env.storage().instance().set(&DataKey::TotalShares, &0_i128);
        env.storage().instance().set(&DataKey::HarvestCount, &0_u64);
        env.storage().instance().set(&DataKey::TotalYieldHarvested, &0_i128);
        env.storage().instance().set(&DataKey::Paused, &false);
    }

    // ── SEP-56 §4: Asset information ─────────────────────────────────────────

    /// Returns the address of the underlying asset token.
    /// Panics if asset not set (SEP-56 deviation from ERC-4626 which never reverts).
    pub fn query_asset(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Asset).expect("asset not set")
    }

    /// Total underlying assets managed by the vault.
    pub fn total_assets(env: Env) -> i128 {
        _total_assets(&env)
    }

    // ── SEP-56 §5: Conversion functions ──────────────────────────────────────

    /// Convert asset amount to shares at current rate (rounds DOWN).
    pub fn convert_to_shares(env: Env, assets: i128) -> i128 {
        _assets_to_shares(assets, _total_assets(&env), _total_shares(&env))
    }

    /// Convert share amount to assets at current rate (rounds DOWN).
    pub fn convert_to_assets(env: Env, shares: i128) -> i128 {
        _shares_to_assets(shares, _total_assets(&env), _total_shares(&env))
    }

    // ── SEP-56 §6: Limits ────────────────────────────────────────────────────

    pub fn max_deposit(_env: Env, _receiver: Address) -> i128 { i128::MAX }
    pub fn max_mint(_env: Env, _receiver: Address) -> i128 { i128::MAX }
    pub fn max_withdraw(env: Env, owner: Address) -> i128 {
        let shares = _balance_of(&env, &owner);
        _shares_to_assets(shares, _total_assets(&env), _total_shares(&env))
    }
    pub fn max_redeem(env: Env, owner: Address) -> i128 {
        _balance_of(&env, &owner)
    }

    // ── SEP-56 §7: Preview functions (read-only simulations) ─────────────────

    pub fn preview_deposit(env: Env, assets: i128) -> i128 {
        _assets_to_shares(assets, _total_assets(&env), _total_shares(&env))
    }
    pub fn preview_mint(env: Env, shares: i128) -> i128 {
        // assets needed to mint exactly `shares` — rounds UP (protects vault)
        let ta = _total_assets(&env);
        let ts = _total_shares(&env);
        let virtual_supply = ts + OFFSET_FACTOR;
        let virtual_assets = ta + 1;
        (shares * virtual_assets + virtual_supply - 1) / virtual_supply
    }
    pub fn preview_withdraw(env: Env, assets: i128) -> i128 {
        // shares burned to withdraw exactly `assets` — rounds UP (protects vault)
        let ta = _total_assets(&env);
        let ts = _total_shares(&env);
        let virtual_supply = ts + OFFSET_FACTOR;
        let virtual_assets = ta + 1;
        (assets * virtual_supply + virtual_assets - 1) / virtual_assets
    }
    pub fn preview_redeem(env: Env, shares: i128) -> i128 {
        _shares_to_assets(shares, _total_assets(&env), _total_shares(&env))
    }

    // ── SEP-56 §8: Deposit / Mint ────────────────────────────────────────────

    /// Deposit `assets` of underlying token, receive vault shares.
    /// `from` must have approved the vault to transfer `assets` on their behalf.
    pub fn deposit(env: Env, assets: i128, receiver: Address, from: Address) -> i128 {
        from.require_auth();
        if assets <= 0 { panic_with_error!(&env, VaultError::ZeroAmount); }
        let paused: bool = env.storage().instance().get(&DataKey::Paused).unwrap_or(false);
        if paused { panic_with_error!(&env, VaultError::VaultPaused); }

        let ta = _total_assets(&env);
        let ts = _total_shares(&env);
        let shares = _assets_to_shares(assets, ta, ts);
        if shares <= 0 { panic_with_error!(&env, VaultError::ZeroAmount); }

        let asset_id: Address = env.storage().instance().get(&DataKey::Asset).unwrap();
        token::Client::new(&env, &asset_id).transfer(&from, &env.current_contract_address(), &assets);

        _mint_shares(&env, &receiver, shares);
        env.storage().instance().set(&DataKey::TotalAssets, &(ta + assets));

        env.events().publish((Symbol::new(&env, "deposit"),), (from, receiver, assets, shares));
        shares
    }

    /// Mint exactly `shares` to receiver, pulling required assets from `from`.
    pub fn mint(env: Env, shares: i128, receiver: Address, from: Address) -> i128 {
        from.require_auth();
        if shares <= 0 { panic_with_error!(&env, VaultError::ZeroAmount); }
        let paused: bool = env.storage().instance().get(&DataKey::Paused).unwrap_or(false);
        if paused { panic_with_error!(&env, VaultError::VaultPaused); }

        let ta = _total_assets(&env);
        let ts = _total_shares(&env);
        // Round UP assets needed (protects vault from rounding drain)
        let virtual_supply = ts + OFFSET_FACTOR;
        let virtual_assets = ta + 1;
        let assets = (shares * virtual_assets + virtual_supply - 1) / virtual_supply;

        let asset_id: Address = env.storage().instance().get(&DataKey::Asset).unwrap();
        token::Client::new(&env, &asset_id).transfer(&from, &env.current_contract_address(), &assets);

        _mint_shares(&env, &receiver, shares);
        env.storage().instance().set(&DataKey::TotalAssets, &(ta + assets));

        env.events().publish((Symbol::new(&env, "deposit"),), (from, receiver, assets, shares));
        assets
    }

    // ── SEP-56 §9: Withdraw / Redeem ─────────────────────────────────────────

    /// Withdraw exactly `assets` of underlying, burning shares from `owner`.
    /// `operator` must be authorized (either owner or an approved spender).
    pub fn withdraw(
        env: Env,
        assets: i128,
        receiver: Address,
        owner: Address,
        operator: Address,
    ) -> i128 {
        operator.require_auth();
        if operator != owner {
            // Check allowance
            let allowance: i128 = env.storage().persistent()
                .get(&DataKey::Allowance(owner.clone(), operator.clone()))
                .unwrap_or(0);
            if allowance < assets { panic_with_error!(&env, VaultError::NotAuthorized); }
            env.storage().persistent()
                .set(&DataKey::Allowance(owner.clone(), operator.clone()), &(allowance - assets));
        }
        if assets <= 0 { panic_with_error!(&env, VaultError::ZeroAmount); }

        let ta = _total_assets(&env);
        let ts = _total_shares(&env);
        // Round UP shares to burn (protects vault)
        let virtual_supply = ts + OFFSET_FACTOR;
        let virtual_assets = ta + 1;
        let shares = (assets * virtual_supply + virtual_assets - 1) / virtual_assets;

        _burn_shares(&env, &owner, shares);
        env.storage().instance().set(&DataKey::TotalAssets, &(ta - assets));

        let asset_id: Address = env.storage().instance().get(&DataKey::Asset).unwrap();
        token::Client::new(&env, &asset_id).transfer(&env.current_contract_address(), &receiver, &assets);

        env.events().publish((Symbol::new(&env, "withdraw"),), (operator, receiver, owner, assets, shares));
        shares
    }

    /// Redeem exactly `shares`, receiving proportional underlying assets.
    pub fn redeem(
        env: Env,
        shares: i128,
        receiver: Address,
        owner: Address,
        operator: Address,
    ) -> i128 {
        operator.require_auth();
        if operator != owner {
            let allowance: i128 = env.storage().persistent()
                .get(&DataKey::Allowance(owner.clone(), operator.clone()))
                .unwrap_or(0);
            if allowance < shares { panic_with_error!(&env, VaultError::NotAuthorized); }
            env.storage().persistent()
                .set(&DataKey::Allowance(owner.clone(), operator.clone()), &(allowance - shares));
        }
        if shares <= 0 { panic_with_error!(&env, VaultError::ZeroAmount); }

        let ta = _total_assets(&env);
        let ts = _total_shares(&env);
        let assets = _shares_to_assets(shares, ta, ts);
        if assets <= 0 { panic_with_error!(&env, VaultError::InsufficientAssets); }

        _burn_shares(&env, &owner, shares);
        env.storage().instance().set(&DataKey::TotalAssets, &(ta - assets));

        let asset_id: Address = env.storage().instance().get(&DataKey::Asset).unwrap();
        token::Client::new(&env, &asset_id).transfer(&env.current_contract_address(), &receiver, &assets);

        env.events().publish((Symbol::new(&env, "withdraw"),), (operator, receiver, owner, assets, shares));
        assets
    }

    // ── SEP-41 Fungible Token Interface (share token) ────────────────────────
    // The vault itself IS the share token per SEP-56.

    pub fn name(env: Env) -> String {
        env.storage().instance().get(&DataKey::Name).unwrap()
    }
    pub fn symbol(env: Env) -> String {
        env.storage().instance().get(&DataKey::Symbol).unwrap()
    }
    pub fn decimals(env: Env) -> u32 {
        let base: u32 = env.storage().instance().get(&DataKey::Decimals).unwrap_or(7);
        base + DECIMALS_OFFSET
    }
    pub fn balance(env: Env, id: Address) -> i128 { _balance_of(&env, &id) }
    pub fn total_supply(env: Env) -> i128 { _total_shares(&env) }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();
        if amount < 0 { panic_with_error!(&env, VaultError::ZeroAmount); }
        let from_bal = _balance_of(&env, &from);
        if from_bal < amount { panic_with_error!(&env, VaultError::InsufficientShares); }
        env.storage().persistent().set(&DataKey::Balance(from.clone()), &(from_bal - amount));
        let to_bal = _balance_of(&env, &to);
        env.storage().persistent().set(&DataKey::Balance(to.clone()), &(to_bal + amount));
        env.events().publish((Symbol::new(&env, "transfer"),), (from, to, amount));
    }

    pub fn transfer_from(env: Env, spender: Address, from: Address, to: Address, amount: i128) {
        spender.require_auth();
        let allowance: i128 = env.storage().persistent()
            .get(&DataKey::Allowance(from.clone(), spender.clone()))
            .unwrap_or(0);
        if allowance < amount { panic_with_error!(&env, VaultError::NotAuthorized); }
        env.storage().persistent()
            .set(&DataKey::Allowance(from.clone(), spender.clone()), &(allowance - amount));
        let from_bal = _balance_of(&env, &from);
        if from_bal < amount { panic_with_error!(&env, VaultError::InsufficientShares); }
        env.storage().persistent().set(&DataKey::Balance(from.clone()), &(from_bal - amount));
        let to_bal = _balance_of(&env, &to);
        env.storage().persistent().set(&DataKey::Balance(to.clone()), &(to_bal + amount));
        env.events().publish((Symbol::new(&env, "transfer"),), (from, to, amount));
    }

    pub fn approve(env: Env, from: Address, spender: Address, amount: i128, _expiration_ledger: u32) {
        from.require_auth();
        env.storage().persistent().set(&DataKey::Allowance(from.clone(), spender.clone()), &amount);
        env.events().publish((Symbol::new(&env, "approve"),), (from, spender, amount));
    }

    pub fn allowance(env: Env, from: Address, spender: Address) -> i128 {
        env.storage().persistent()
            .get(&DataKey::Allowance(from, spender))
            .unwrap_or(0)
    }

    // ── Yield management ─────────────────────────────────────────────────────

    /// Admin harvests yield and compounds it into the vault.
    /// Yield increases `total_assets` without minting new shares →
    /// each share becomes worth more underlying asset.
    /// Performance fee (if configured) is deducted before compounding.
    pub fn harvest(env: Env, gross_yield: i128) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        if gross_yield <= 0 { return; }

        let fee_bps: u32 = env.storage().instance().get(&DataKey::PerformanceFeeBps).unwrap_or(0);
        let fee_amount = gross_yield * fee_bps as i128 / 10_000;
        let net_yield = gross_yield - fee_amount;

        if fee_amount > 0 {
            let fee_collector: Address = env.storage().instance().get(&DataKey::FeeCollector).unwrap();
            let asset_id: Address = env.storage().instance().get(&DataKey::Asset).unwrap();
            token::Client::new(&env, &asset_id)
                .transfer(&env.current_contract_address(), &fee_collector, &fee_amount);
        }

        let ta = _total_assets(&env);
        env.storage().instance().set(&DataKey::TotalAssets, &(ta + net_yield));

        let count: u64 = env.storage().instance().get(&DataKey::HarvestCount).unwrap_or(0);
        env.storage().instance().set(&DataKey::HarvestCount, &(count + 1));
        let total: i128 = env.storage().instance().get(&DataKey::TotalYieldHarvested).unwrap_or(0);
        env.storage().instance().set(&DataKey::TotalYieldHarvested, &(total + net_yield));

        env.events().publish((Symbol::new(&env, "harvested"),), (net_yield, fee_amount));
    }

    pub fn pause(env: Env) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        env.storage().instance().set(&DataKey::Paused, &true);
    }
    pub fn resume(env: Env) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        env.storage().instance().set(&DataKey::Paused, &false);
    }

    pub fn get_harvest_count(env: Env) -> u64 {
        env.storage().instance().get(&DataKey::HarvestCount).unwrap_or(0)
    }
    pub fn get_total_yield_harvested(env: Env) -> i128 {
        env.storage().instance().get(&DataKey::TotalYieldHarvested).unwrap_or(0)
    }
    pub fn is_paused(env: Env) -> bool {
        env.storage().instance().get(&DataKey::Paused).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::token::StellarAssetClient;
    use soroban_sdk::{Env, String};

    fn setup() -> (Env, Address, Address, Address, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let fc    = Address::generate(&env);
        let user  = Address::generate(&env);
        let asset = env.register_stellar_asset_contract(admin.clone());
        let cid   = env.register_contract(None, YieldVault);
        (env, admin, fc, user, asset, cid)
    }

    fn init(client: &YieldVaultClient, env: &Env, admin: &Address, asset: &Address, fc: &Address) {
        client.initialize(
            admin, asset, fc,
            &String::from_str(env, "YieldRoutes USDC"),
            &String::from_str(env, "yrUSDC"),
            &7_u32, &1_000_u32, // 10% performance fee
        );
    }

    #[test]
    fn test_sep56_deposit_and_redeem() {
        let (env, admin, fc, user, asset, cid) = setup();
        StellarAssetClient::new(&env, &asset).mint(&user, &10_000_000_i128);
        let client = YieldVaultClient::new(&env, &cid);
        init(&client, &env, &admin, &asset, &fc);

        // query_asset() must return asset address
        assert_eq!(client.query_asset(), asset);

        // Deposit 1,000,000 units → receive shares
        let shares = client.deposit(&1_000_000_i128, &user, &user);
        assert!(shares > 0);
        assert_eq!(client.total_assets(), 1_000_000);

        // preview_redeem should match actual redeem
        let preview = client.preview_redeem(&shares);
        let redeemed = client.redeem(&shares, &user, &user, &user);
        assert_eq!(redeemed, preview);
        assert_eq!(client.total_assets(), 0);
    }

    #[test]
    fn test_sep56_harvest_increases_share_value() {
        let (env, admin, fc, user, asset, cid) = setup();
        StellarAssetClient::new(&env, &asset).mint(&user, &10_000_000_i128);
        // Mint yield to vault for testing
        StellarAssetClient::new(&env, &asset).mint(&cid, &1_000_000_i128);
        let client = YieldVaultClient::new(&env, &cid);
        init(&client, &env, &admin, &asset, &fc);

        // Deposit 1M
        let shares = client.deposit(&1_000_000_i128, &user, &user);
        let assets_before = client.convert_to_assets(&shares);

        // Harvest 100k yield (10% fee → 90k net)
        client.harvest(&100_000_i128);

        let assets_after = client.convert_to_assets(&shares);
        assert!(assets_after > assets_before, "harvest must increase share value");
        assert_eq!(client.get_harvest_count(), 1);
    }

    #[test]
    fn test_sep56_mint_function() {
        let (env, admin, fc, user, asset, cid) = setup();
        StellarAssetClient::new(&env, &asset).mint(&user, &10_000_000_i128);
        let client = YieldVaultClient::new(&env, &cid);
        init(&client, &env, &admin, &asset, &fc);

        // Mint exactly 1000 shares
        let target_shares = 1_000_i128;
        let preview_cost = client.preview_mint(&target_shares);
        let actual_cost = client.mint(&target_shares, &user, &user);
        // Actual cost should be ≥ preview (rounding rounds up for vault)
        assert!(actual_cost >= preview_cost);
        assert_eq!(client.balance(&user), target_shares);
    }

    #[test]
    fn test_sep56_withdraw_function() {
        let (env, admin, fc, user, asset, cid) = setup();
        StellarAssetClient::new(&env, &asset).mint(&user, &10_000_000_i128);
        let client = YieldVaultClient::new(&env, &cid);
        init(&client, &env, &admin, &asset, &fc);

        client.deposit(&5_000_000_i128, &user, &user);
        let preview_shares = client.preview_withdraw(&1_000_000_i128);
        let shares_burned = client.withdraw(&1_000_000_i128, &user, &user, &user);
        assert_eq!(shares_burned, preview_shares);
    }

    #[test]
    fn test_sep41_transfer() {
        let (env, admin, fc, user, asset, cid) = setup();
        let user2 = Address::generate(&env);
        StellarAssetClient::new(&env, &asset).mint(&user, &5_000_000_i128);
        let client = YieldVaultClient::new(&env, &cid);
        init(&client, &env, &admin, &asset, &fc);

        let shares = client.deposit(&5_000_000_i128, &user, &user);
        client.transfer(&user, &user2, &(shares / 2));
        assert_eq!(client.balance(&user),  shares / 2);
        assert_eq!(client.balance(&user2), shares / 2);
    }

    #[test]
    fn test_inflation_attack_protected() {
        // Verify virtual offset prevents inflation attack
        let (env, admin, fc, _user, asset, cid) = setup();
        let attacker = Address::generate(&env);
        let victim   = Address::generate(&env);
        StellarAssetClient::new(&env, &asset).mint(&attacker, &100_000_000_i128);
        StellarAssetClient::new(&env, &asset).mint(&victim,   &100_000_i128);
        let client = YieldVaultClient::new(&env, &cid);
        init(&client, &env, &admin, &asset, &fc);

        // Attacker deposits 1 unit
        client.deposit(&1_i128, &attacker, &attacker);
        // "Donate" large amount directly (simulated by harvest)
        client.harvest(&10_000_000_i128);
        // Victim deposits 100k — should still get > 0 shares due to offset
        let victim_shares = client.deposit(&100_000_i128, &victim, &victim);
        assert!(victim_shares > 0, "inflation attack protection failed — victim got 0 shares");
    }
}
