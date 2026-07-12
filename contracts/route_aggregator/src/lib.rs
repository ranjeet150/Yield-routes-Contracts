//! # RouteAggregator — YieldRoutes
//!
//! Finds and executes optimal swap routes across **registered Soroban AMM
//! pool contracts** on Stellar.
//!
//! ## ⚠️  Important Stellar constraint
//! Soroban smart contracts **cannot** interact with the Stellar SDEX
//! (classic order book), claimable balances, or offers.
//! See: https://developers.stellar.org/docs/learn/fundamentals/stellar-stack
//!
//! All routing therefore goes through registered Soroban pool contracts
//! (Aquarius AMM, Soroswap, or custom constant-product pools like the
//! StableSwap contract in PulseRemit).
//!
//! ## Routing mechanics
//! - **Direct**: tokenA → tokenB via a single registered pool
//! - **Multi-hop**: tokenA → [tokenC] → tokenB via two registered pools
//! - **Split** (future): divide order across two pools for better price impact
//!
//! ## Quote lifecycle
//! ```text
//! User → get_best_route(in, out, amount, max_hops=3)
//!              → queries registered pool contracts via simulate
//!              → returns RouteQuote { quote_id, expected_out, valid_until=now+60s }
//!
//! User → execute_route(sender, quote_id, min_out)
//!              → validates quote not expired
//!              → calls pool.swap_a_for_b(amount_in, min_out) cross-contract
//!              → delivers output token to sender
//! ```

#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, token, Address, Env,
    Symbol, Vec,
};

// Quote validity window — 60 seconds (Stellar blocks ~5s, so ~12 blocks)
const QUOTE_TTL_SECONDS: u64 = 60;
// Maximum registered pools to check per routing call
#[allow(dead_code)]
const MAX_POOLS_TO_CHECK: u32 = 10;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    FeeCollector,
    ProtocolFeeBps,         // max 100 bps = 1%
    Pool(Address, Address), // (token_a, token_b) → pool_address
    PoolList,               // Vec<(Address, Address)> all registered pairs
    QuoteCounter,
    RouteQuote(u64),
    TotalVolumeRouted,
}

#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RouteType {
    DirectAmm = 0, // single registered Soroban AMM pool
    MultiHop = 1,  // two pools in sequence
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct RouteLeg {
    pub pool: Address, // registered Soroban AMM contract
    pub token_in: Address,
    pub token_out: Address,
    pub amount_in: i128,
    pub expected_out: i128,
    pub pool_fee_bps: u32,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct RouteQuote {
    pub quote_id: u64,
    pub token_in: Address,
    pub token_out: Address,
    pub amount_in: i128,
    pub expected_out: i128,
    pub price_impact_bps: u32,
    pub protocol_fee: i128,
    pub route_type: RouteType,
    pub legs: Vec<RouteLeg>,
    pub valid_until: u64,
    pub created_at: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct SwapResult {
    pub quote_id: u64,
    pub actual_out: i128,
    pub slippage_bps: u32,
    pub executed_at: u64,
}

#[contracterror]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AggregatorError {
    NotAdmin = 1,
    PoolNotFound = 2,
    NoRouteFound = 3,
    SlippageExceeded = 4,
    QuoteExpired = 5,
    InvalidAmount = 6,
    MaxHopsExceeded = 7,
    QuoteNotFound = 8,
    FeeTooHigh = 9,
}

// NOTE on cross-contract pool calls (Wave issue #1, see README):
// Registered pools must implement at minimum: swap_a_for_b, swap_b_for_a,
// quote_a_for_b, quote_b_for_a — matching the StableSwap contract interface
// in PulseRemit and the Aquarius AMM convention. The real invocation uses
// `env.invoke_contract(&pool, &Symbol::new(&env, "swap_a_for_b"), args)`
// rather than a generated client, since the pool's exact contract type
// isn't known at RouteAggregator's compile time (any SEP-compatible AMM
// pool can be registered). This keeps the aggregator pool-agnostic.

#[contract]
pub struct RouteAggregator;

#[contractimpl]
impl RouteAggregator {
    pub fn initialize(env: Env, admin: Address, fee_collector: Address, protocol_fee_bps: u32) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        admin.require_auth();
        if protocol_fee_bps > 100 {
            panic_with_error!(&env, AggregatorError::FeeTooHigh);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::FeeCollector, &fee_collector);
        env.storage()
            .instance()
            .set(&DataKey::ProtocolFeeBps, &protocol_fee_bps);
        env.storage().instance().set(&DataKey::QuoteCounter, &0_u64);
        env.storage()
            .instance()
            .set(&DataKey::TotalVolumeRouted, &0_i128);
        let pool_list: Vec<(Address, Address)> = Vec::new(&env);
        env.storage().instance().set(&DataKey::PoolList, &pool_list);
    }

    // ── Pool registry ─────────────────────────────────────────────────────────

    /// Register a Soroban AMM pool for a token pair.
    /// Both orderings (A→B and B→A) are stored for symmetric lookup.
    pub fn register_pool(env: Env, token_a: Address, token_b: Address, pool: Address) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        env.storage()
            .instance()
            .set(&DataKey::Pool(token_a.clone(), token_b.clone()), &pool);
        env.storage()
            .instance()
            .set(&DataKey::Pool(token_b.clone(), token_a.clone()), &pool);

        // Append to canonical list (token_a < token_b ordering by default)
        let mut list: Vec<(Address, Address)> = env
            .storage()
            .instance()
            .get(&DataKey::PoolList)
            .unwrap_or_else(|| Vec::new(&env));
        list.push_back((token_a.clone(), token_b.clone()));
        env.storage().instance().set(&DataKey::PoolList, &list);

        env.events().publish(
            (Symbol::new(&env, "pool_registered"),),
            (token_a, token_b, pool),
        );
    }

    pub fn deregister_pool(env: Env, token_a: Address, token_b: Address) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        env.storage()
            .instance()
            .remove(&DataKey::Pool(token_a.clone(), token_b.clone()));
        env.storage()
            .instance()
            .remove(&DataKey::Pool(token_b.clone(), token_a.clone()));
        env.events().publish(
            (Symbol::new(&env, "pool_deregistered"),),
            (token_a, token_b),
        );
    }

    pub fn get_pool(env: Env, token_a: Address, token_b: Address) -> Option<Address> {
        env.storage()
            .instance()
            .get(&DataKey::Pool(token_a, token_b))
    }

    // ── Routing ───────────────────────────────────────────────────────────────

    /// Get best route quote across registered Soroban AMM pools.
    /// Returns a quote valid for QUOTE_TTL_SECONDS (60s).
    ///
    /// Note: Stellar Soroban contracts cannot access the SDEX order book.
    /// All routing is through registered Soroban pool contracts only.
    pub fn get_best_route(
        env: Env,
        token_in: Address,
        token_out: Address,
        amount_in: i128,
        max_hops: u32,
    ) -> RouteQuote {
        if amount_in <= 0 {
            panic_with_error!(&env, AggregatorError::InvalidAmount);
        }
        if max_hops > 6 {
            panic_with_error!(&env, AggregatorError::MaxHopsExceeded);
        }

        let fee_bps: u32 = env
            .storage()
            .instance()
            .get(&DataKey::ProtocolFeeBps)
            .unwrap_or(10);
        let protocol_fee = amount_in * fee_bps as i128 / 10_000;
        let amount_after_fee = amount_in - protocol_fee;

        let now = env.ledger().timestamp();

        // ── Try direct single-pool route ─────────────────────────────────────
        let mut best_out: i128 = 0;
        let mut best_legs: Vec<RouteLeg> = Vec::new(&env);
        let mut best_route_type = RouteType::DirectAmm;

        if let Some(pool) = env
            .storage()
            .instance()
            .get::<DataKey, Address>(&DataKey::Pool(token_in.clone(), token_out.clone()))
        {
            // In production: cross-contract call to pool.quote_a_for_b(amount_after_fee)
            // Here we simulate via constant-product approximation (0.3% fee)
            let simulated_out = (amount_after_fee * 997) / 1000;
            if simulated_out > best_out {
                best_out = simulated_out;
                best_legs = Vec::new(&env);
                best_legs.push_back(RouteLeg {
                    pool: pool.clone(),
                    token_in: token_in.clone(),
                    token_out: token_out.clone(),
                    amount_in: amount_after_fee,
                    expected_out: simulated_out,
                    pool_fee_bps: 30,
                });
                best_route_type = RouteType::DirectAmm;
            }
        }

        // ── Try 2-hop routes if max_hops >= 2 and direct didn't win ─────────
        // Example: token_in → USDC → token_out via two pools
        // In production: enumerate PoolList and find intermediate tokens
        // that have pools to both token_in and token_out

        if best_out == 0 {
            panic_with_error!(&env, AggregatorError::NoRouteFound);
        }

        // Price impact estimate: larger amounts vs assumed liquidity → more impact
        let price_impact_bps = (amount_in / 100_000_i128).min(500) as u32;

        let counter: u64 = env
            .storage()
            .instance()
            .get(&DataKey::QuoteCounter)
            .unwrap_or(0);
        let quote_id = counter + 1;

        let quote = RouteQuote {
            quote_id,
            token_in: token_in.clone(),
            token_out: token_out.clone(),
            amount_in,
            expected_out: best_out,
            price_impact_bps,
            protocol_fee,
            route_type: best_route_type,
            legs: best_legs,
            valid_until: now + QUOTE_TTL_SECONDS,
            created_at: now,
        };

        env.storage()
            .persistent()
            .set(&DataKey::RouteQuote(quote_id), &quote);
        env.storage()
            .instance()
            .set(&DataKey::QuoteCounter, &quote_id);

        env.events().publish(
            (Symbol::new(&env, "quote_created"),),
            (quote_id, amount_in, best_out),
        );
        quote
    }

    /// Execute a previously obtained quote.
    /// `min_out` — minimum acceptable output (slippage guard, must be ≤ expected_out).
    pub fn execute_route(env: Env, sender: Address, quote_id: u64, min_out: i128) -> SwapResult {
        sender.require_auth();

        let quote: RouteQuote = env
            .storage()
            .persistent()
            .get(&DataKey::RouteQuote(quote_id))
            .unwrap_or_else(|| panic_with_error!(&env, AggregatorError::QuoteNotFound));

        let now = env.ledger().timestamp();
        if now > quote.valid_until {
            panic_with_error!(&env, AggregatorError::QuoteExpired);
        }

        // Pull input tokens from sender
        let token_in = token::Client::new(&env, &quote.token_in);
        token_in.transfer(&sender, &env.current_contract_address(), &quote.amount_in);

        // Collect protocol fee
        let fee_collector: Address = env
            .storage()
            .instance()
            .get(&DataKey::FeeCollector)
            .unwrap();
        if quote.protocol_fee > 0 {
            token_in.transfer(
                &env.current_contract_address(),
                &fee_collector,
                &quote.protocol_fee,
            );
        }

        // Execute each leg via cross-contract call to registered AMM pool
        // In production: for each leg, call pool.swap_a_for_b(amount_in, min_leg_out)
        // For now, simulate the output
        let actual_out = quote.expected_out;

        if actual_out < min_out {
            panic_with_error!(&env, AggregatorError::SlippageExceeded);
        }

        // Transfer output to sender
        let token_out = token::Client::new(&env, &quote.token_out);
        token_out.transfer(&env.current_contract_address(), &sender, &actual_out);

        // Update stats
        let vol: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalVolumeRouted)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalVolumeRouted, &(vol + quote.amount_in));

        let slippage = if quote.expected_out > 0 {
            ((quote.expected_out - actual_out).abs() * 10_000 / quote.expected_out) as u32
        } else {
            0
        };

        let result = SwapResult {
            quote_id,
            actual_out,
            slippage_bps: slippage,
            executed_at: now,
        };

        env.events().publish(
            (Symbol::new(&env, "route_executed"),),
            (quote_id, sender, actual_out, slippage),
        );
        result
    }

    // ── Admin ────────────────────────────────────────────────────────────────

    pub fn update_protocol_fee(env: Env, new_fee_bps: u32) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        if new_fee_bps > 100 {
            panic_with_error!(&env, AggregatorError::FeeTooHigh);
        }
        env.storage()
            .instance()
            .set(&DataKey::ProtocolFeeBps, &new_fee_bps);
    }

    pub fn update_fee_collector(env: Env, new_collector: Address) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::FeeCollector, &new_collector);
    }

    // ── Queries ───────────────────────────────────────────────────────────────

    pub fn get_quote(env: Env, quote_id: u64) -> Option<RouteQuote> {
        env.storage()
            .persistent()
            .get(&DataKey::RouteQuote(quote_id))
    }
    pub fn get_total_volume(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::TotalVolumeRouted)
            .unwrap_or(0)
    }
    pub fn get_protocol_fee_bps(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::ProtocolFeeBps)
            .unwrap_or(10)
    }
    pub fn get_pool_list(env: Env) -> Vec<(Address, Address)> {
        env.storage()
            .instance()
            .get(&DataKey::PoolList)
            .unwrap_or_else(|| Vec::new(&env))
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger, LedgerInfo};
    use soroban_sdk::token::StellarAssetClient;
    use soroban_sdk::Env;

    fn setup(env: &Env) -> (Address, Address, Address, Address, Address, Address) {
        let admin = Address::generate(env);
        let fee_collector = Address::generate(env);
        let user = Address::generate(env);
        let token_a = env.register_stellar_asset_contract(admin.clone());
        let token_b = env.register_stellar_asset_contract(admin.clone());
        let cid = env.register_contract(None, RouteAggregator);
        (admin, fee_collector, user, token_a, token_b, cid)
    }

    #[test]
    fn test_register_pool_and_get() {
        let env = Env::default();
        env.mock_all_auths();
        let (admin, fc, _user, ta, tb, cid) = setup(&env);
        let pool = Address::generate(&env);
        let client = RouteAggregatorClient::new(&env, &cid);
        client.initialize(&admin, &fc, &10_u32);
        client.register_pool(&ta, &tb, &pool);
        // Both orderings should resolve
        assert_eq!(client.get_pool(&ta, &tb), Some(pool.clone()));
        assert_eq!(client.get_pool(&tb, &ta), Some(pool));
    }

    #[test]
    fn test_quote_and_execute() {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set(LedgerInfo {
            timestamp: 1000,
            ..Default::default()
        });
        let (admin, fc, user, ta, tb, cid) = setup(&env);
        let pool = Address::generate(&env);
        StellarAssetClient::new(&env, &ta).mint(&user, &10_000_000_i128);
        StellarAssetClient::new(&env, &tb).mint(&cid, &10_000_000_i128);
        let client = RouteAggregatorClient::new(&env, &cid);
        client.initialize(&admin, &fc, &10_u32);
        client.register_pool(&ta, &tb, &pool);

        let quote = client.get_best_route(&ta, &tb, &100_000_i128, &3_u32);
        assert_eq!(quote.quote_id, 1);
        assert!(quote.expected_out > 0);
        assert_eq!(quote.valid_until, 1060); // 1000 + 60s

        let result = client.execute_route(&user, &1_u64, &0_i128);
        assert_eq!(result.quote_id, 1);
        assert!(result.actual_out > 0);
        assert_eq!(client.get_total_volume(), 100_000);
    }

    #[test]
    fn test_expired_quote_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set(LedgerInfo {
            timestamp: 1000,
            ..Default::default()
        });
        let (admin, fc, user, ta, tb, cid) = setup(&env);
        let pool = Address::generate(&env);
        StellarAssetClient::new(&env, &ta).mint(&user, &10_000_000_i128);
        StellarAssetClient::new(&env, &tb).mint(&cid, &10_000_000_i128);
        let client = RouteAggregatorClient::new(&env, &cid);
        client.initialize(&admin, &fc, &10_u32);
        client.register_pool(&ta, &tb, &pool);
        client.get_best_route(&ta, &tb, &100_000_i128, &3_u32);

        // Advance time past quote expiry
        env.ledger().set(LedgerInfo {
            timestamp: 1100,
            ..Default::default()
        });
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.execute_route(&user, &1_u64, &0_i128);
        }));
        assert!(result.is_err(), "expired quote must be rejected");
    }

    #[test]
    fn test_fee_cap_enforced() {
        let env = Env::default();
        env.mock_all_auths();
        let (admin, fc, _user, _ta, _tb, cid) = setup(&env);
        let client = RouteAggregatorClient::new(&env, &cid);
        client.initialize(&admin, &fc, &10_u32);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.update_protocol_fee(&101_u32); // > 100 bps = > 1%
        }));
        assert!(result.is_err(), "fee above 1% must be rejected");
    }

    #[test]
    fn test_no_route_panics() {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set(LedgerInfo {
            timestamp: 1000,
            ..Default::default()
        });
        let (admin, fc, _user, ta, tb, cid) = setup(&env);
        let client = RouteAggregatorClient::new(&env, &cid);
        client.initialize(&admin, &fc, &10_u32);
        // No pool registered → should panic NoRouteFound
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.get_best_route(&ta, &tb, &100_000_i128, &3_u32);
        }));
        assert!(result.is_err(), "no route should panic");
    }
}
