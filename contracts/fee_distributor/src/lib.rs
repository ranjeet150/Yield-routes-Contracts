//! # FeeDistributor — YieldRoutes
//!
//! Collects protocol fees from RouteAggregator and YieldVault,
//! then distributes them to stakeholders according to configured splits:
//! - Treasury: covers protocol operating costs
//! - Yield Vault: re-deposited to benefit all depositors
//! - LP Rewards: distributed to liquidity providers
//!
//! Distribution is triggered permissionlessly once per epoch (24h).

#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype,
    Address, Env, Symbol, token,
};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    TokenId,
    Treasury,
    YieldVault,
    LpRewards,
    TreasurySplitBps,
    VaultSplitBps,
    LpSplitBps,
    TotalDistributed,
    LastDistributionTs,
    DistributionCount,
    EpochDurationSeconds,
}

#[contract]
pub struct FeeDistributor;

#[contractimpl]
impl FeeDistributor {

    pub fn initialize(
        env: Env,
        admin: Address,
        token_id: Address,
        treasury: Address,
        yield_vault: Address,
        lp_rewards: Address,
        treasury_bps: u32,
        vault_bps: u32,
        lp_bps: u32,
    ) {
        if env.storage().instance().has(&DataKey::Admin) { panic!("already initialized"); }
        admin.require_auth();
        assert!(treasury_bps + vault_bps + lp_bps == 10_000, "splits must sum to 100%");
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::TokenId, &token_id);
        env.storage().instance().set(&DataKey::Treasury, &treasury);
        env.storage().instance().set(&DataKey::YieldVault, &yield_vault);
        env.storage().instance().set(&DataKey::LpRewards, &lp_rewards);
        env.storage().instance().set(&DataKey::TreasurySplitBps, &treasury_bps);
        env.storage().instance().set(&DataKey::VaultSplitBps, &vault_bps);
        env.storage().instance().set(&DataKey::LpSplitBps, &lp_bps);
        env.storage().instance().set(&DataKey::TotalDistributed, &0_i128);
        env.storage().instance().set(&DataKey::DistributionCount, &0_u64);
        env.storage().instance().set(&DataKey::EpochDurationSeconds, &86_400_u64); // 24h
    }

    /// Distribute accumulated fees. Callable by anyone once per epoch.
    pub fn distribute(env: Env, amount: i128) {
        if amount <= 0 { return; }

        let last: u64 = env.storage().instance().get(&DataKey::LastDistributionTs).unwrap_or(0);
        let epoch: u64 = env.storage().instance().get(&DataKey::EpochDurationSeconds).unwrap();
        let now = env.ledger().timestamp();
        if now < last + epoch { panic!("distribution epoch not elapsed"); }

        let token_id: Address = env.storage().instance().get(&DataKey::TokenId).unwrap();
        let token = token::Client::new(&env, &token_id);
        let treasury_bps: u32 = env.storage().instance().get(&DataKey::TreasurySplitBps).unwrap();
        let vault_bps: u32 = env.storage().instance().get(&DataKey::VaultSplitBps).unwrap();
        let lp_bps: u32 = env.storage().instance().get(&DataKey::LpSplitBps).unwrap();

        let to_treasury = amount * treasury_bps as i128 / 10_000;
        let to_vault    = amount * vault_bps    as i128 / 10_000;
        let to_lp       = amount - to_treasury - to_vault;

        let treasury: Address = env.storage().instance().get(&DataKey::Treasury).unwrap();
        let vault: Address    = env.storage().instance().get(&DataKey::YieldVault).unwrap();
        let lp: Address       = env.storage().instance().get(&DataKey::LpRewards).unwrap();

        if to_treasury > 0 { token.transfer(&env.current_contract_address(), &treasury, &to_treasury); }
        if to_vault    > 0 { token.transfer(&env.current_contract_address(), &vault,    &to_vault); }
        if to_lp       > 0 { token.transfer(&env.current_contract_address(), &lp,       &to_lp); }

        let total: i128 = env.storage().instance().get(&DataKey::TotalDistributed).unwrap_or(0);
        env.storage().instance().set(&DataKey::TotalDistributed, &(total + amount));
        env.storage().instance().set(&DataKey::LastDistributionTs, &now);
        let count: u64 = env.storage().instance().get(&DataKey::DistributionCount).unwrap_or(0);
        env.storage().instance().set(&DataKey::DistributionCount, &(count + 1));

        env.events().publish(
            (Symbol::new(&env, "distributed"),),
            (amount, to_treasury, to_vault, to_lp),
        );
    }

    pub fn get_total_distributed(env: Env) -> i128 {
        env.storage().instance().get(&DataKey::TotalDistributed).unwrap_or(0)
    }

    pub fn get_distribution_count(env: Env) -> u64 {
        env.storage().instance().get(&DataKey::DistributionCount).unwrap_or(0)
    }

    pub fn update_splits(env: Env, treasury_bps: u32, vault_bps: u32, lp_bps: u32) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        assert!(treasury_bps + vault_bps + lp_bps == 10_000, "must sum to 100%");
        env.storage().instance().set(&DataKey::TreasurySplitBps, &treasury_bps);
        env.storage().instance().set(&DataKey::VaultSplitBps, &vault_bps);
        env.storage().instance().set(&DataKey::LpSplitBps, &lp_bps);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger, LedgerInfo};
    use soroban_sdk::token::StellarAssetClient;
    use soroban_sdk::Env;

    #[test]
    fn test_distribute_splits() {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set(LedgerInfo { timestamp: 0, ..Default::default() });

        let admin     = Address::generate(&env);
        let treasury  = Address::generate(&env);
        let vault_addr= Address::generate(&env);
        let lp        = Address::generate(&env);
        let token_id  = env.register_stellar_asset_contract(admin.clone());

        StellarAssetClient::new(&env, &token_id).mint(&admin, &10_000_i128);

        let cid = env.register_contract(None, FeeDistributor);
        let client = FeeDistributorClient::new(&env, &cid);

        // Mint fees to distributor
        StellarAssetClient::new(&env, &token_id).mint(&cid, &10_000_i128);

        client.initialize(
            &admin, &token_id, &treasury, &vault_addr, &lp,
            &5_000_u32,   // 50% treasury
            &3_000_u32,   // 30% vault
            &2_000_u32,   // 20% LP
        );

        // Advance time past one epoch
        env.ledger().set(LedgerInfo { timestamp: 90_000, ..Default::default() });
        client.distribute(&10_000_i128);

        assert_eq!(client.get_total_distributed(), 10_000);
        assert_eq!(client.get_distribution_count(), 1);
    }

    #[test]
    fn test_epoch_gate() {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set(LedgerInfo { timestamp: 0, ..Default::default() });
        let admin = Address::generate(&env);
        let token_id = env.register_stellar_asset_contract(admin.clone());
        let cid = env.register_contract(None, FeeDistributor);
        let client = FeeDistributorClient::new(&env, &cid);
        StellarAssetClient::new(&env, &token_id).mint(&cid, &100_000_i128);

        let t = Address::generate(&env);
        let v = Address::generate(&env);
        let l = Address::generate(&env);
        client.initialize(&admin, &token_id, &t, &v, &l, &5_000_u32, &3_000_u32, &2_000_u32);

        // First call at t=90000 (past 24h) should work
        env.ledger().set(LedgerInfo { timestamp: 90_000, ..Default::default() });
        client.distribute(&1_000_i128);

        // Second call immediately should fail (epoch not elapsed)
        let result = std::panic::catch_unwind(|| client.distribute(&1_000_i128));
        assert!(result.is_err());
    }
}
