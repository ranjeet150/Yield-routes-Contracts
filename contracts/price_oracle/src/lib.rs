//! # PriceOracle — YieldRoutes
//!
//! On-chain TWAP (time-weighted average price) oracle. Authorized price
//! reporters (off-chain bots reading Stellar DEX prices) submit price
//! updates. The oracle computes a rolling TWAP to reduce manipulation risk.
//!
//! Used by RouteAggregator and YieldVault to:
//! - Validate that swap quotes are within acceptable bounds
//! - Compute fair value for strategy allocation decisions

#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Symbol, Vec};

const TWAP_WINDOW_SECONDS: u64 = 3_600; // 1h TWAP
const MAX_PRICE_SAMPLES: u32 = 20;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Reporter(Address),
    PriceSamples(Address, Address), // (base, quote) → Vec<PriceSample>
    LatestPrice(Address, Address),
    MaxDeviation,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct PriceSample {
    pub price: i128, // price in quote token per base token (scaled 1e9)
    pub timestamp: u64,
    pub reporter: Address,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct PriceData {
    pub latest_price: i128,
    pub twap_price: i128,
    pub sample_count: u32,
    pub oldest_sample_ts: u64,
    pub newest_sample_ts: u64,
}

#[contract]
pub struct PriceOracle;

#[contractimpl]
impl PriceOracle {
    pub fn initialize(env: Env, admin: Address, max_deviation_bps: u32) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::MaxDeviation, &max_deviation_bps);
    }

    pub fn add_reporter(env: Env, reporter: Address) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::Reporter(reporter.clone()), &true);
        env.events()
            .publish((Symbol::new(&env, "reporter_added"),), (reporter,));
    }

    pub fn remove_reporter(env: Env, reporter: Address) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        env.storage()
            .instance()
            .remove(&DataKey::Reporter(reporter));
    }

    /// Submit a new price sample. Only authorized reporters can call this.
    pub fn submit_price(
        env: Env,
        reporter: Address,
        base_token: Address,
        quote_token: Address,
        price: i128, // price scaled by 1e9 (e.g. 1.05 USDC/XLM = 1_050_000_000)
    ) {
        reporter.require_auth();
        let is_reporter: bool = env
            .storage()
            .instance()
            .get(&DataKey::Reporter(reporter.clone()))
            .unwrap_or(false);
        if !is_reporter {
            panic!("not an authorized reporter");
        }
        if price <= 0 {
            panic!("price must be positive");
        }

        let sample = PriceSample {
            price,
            timestamp: env.ledger().timestamp(),
            reporter: reporter.clone(),
        };

        let samples: Vec<PriceSample> = env
            .storage()
            .persistent()
            .get(&DataKey::PriceSamples(
                base_token.clone(),
                quote_token.clone(),
            ))
            .unwrap_or_else(|| Vec::new(&env));

        // Keep only recent samples within TWAP window
        let now = env.ledger().timestamp();
        let cutoff = now.saturating_sub(TWAP_WINDOW_SECONDS);
        let mut filtered = Vec::new(&env);
        for s in samples.iter() {
            if s.timestamp >= cutoff {
                filtered.push_back(s);
            }
        }
        // Cap at MAX_PRICE_SAMPLES
        if filtered.len() >= MAX_PRICE_SAMPLES {
            // Remove oldest: create new vec without first element
            let mut trimmed = Vec::new(&env);
            for (i, s) in filtered.iter().enumerate() {
                if i > 0 {
                    trimmed.push_back(s);
                }
            }
            filtered = trimmed;
        }
        filtered.push_back(sample);

        env.storage().persistent().set(
            &DataKey::PriceSamples(base_token.clone(), quote_token.clone()),
            &filtered,
        );
        env.storage().persistent().set(
            &DataKey::LatestPrice(base_token.clone(), quote_token.clone()),
            &price,
        );

        env.events().publish(
            (Symbol::new(&env, "price_submitted"),),
            (base_token, quote_token, price, reporter),
        );
    }

    /// Get the TWAP price over the configured window.
    pub fn get_twap(env: Env, base_token: Address, quote_token: Address) -> i128 {
        let samples: Vec<PriceSample> = env
            .storage()
            .persistent()
            .get(&DataKey::PriceSamples(base_token, quote_token))
            .unwrap_or_else(|| Vec::new(&env));

        if samples.is_empty() {
            return 0;
        }

        let sum: i128 = samples.iter().map(|s| s.price).sum();
        sum / samples.len() as i128
    }

    pub fn get_latest_price(env: Env, base_token: Address, quote_token: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::LatestPrice(base_token, quote_token))
            .unwrap_or(0)
    }

    pub fn get_price_data(env: Env, base_token: Address, quote_token: Address) -> PriceData {
        let samples: Vec<PriceSample> = env
            .storage()
            .persistent()
            .get(&DataKey::PriceSamples(
                base_token.clone(),
                quote_token.clone(),
            ))
            .unwrap_or_else(|| Vec::new(&env));

        if samples.is_empty() {
            return PriceData {
                latest_price: 0,
                twap_price: 0,
                sample_count: 0,
                oldest_sample_ts: 0,
                newest_sample_ts: 0,
            };
        }

        let sum: i128 = samples.iter().map(|s| s.price).sum();
        let twap = sum / samples.len() as i128;
        let latest = env
            .storage()
            .persistent()
            .get(&DataKey::LatestPrice(base_token, quote_token))
            .unwrap_or(0);

        PriceData {
            latest_price: latest,
            twap_price: twap,
            sample_count: samples.len(),
            oldest_sample_ts: samples.get(0).map(|s| s.timestamp).unwrap_or(0),
            newest_sample_ts: samples
                .get(samples.len() - 1)
                .map(|s| s.timestamp)
                .unwrap_or(0),
        }
    }

    pub fn is_reporter(env: Env, address: Address) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Reporter(address))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger, LedgerInfo};
    use soroban_sdk::Env;

    #[test]
    fn test_submit_and_twap() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let reporter = Address::generate(&env);
        let base = Address::generate(&env);
        let quote = Address::generate(&env);
        let cid = env.register_contract(None, PriceOracle);
        let client = PriceOracleClient::new(&env, &cid);
        client.initialize(&admin, &200_u32);
        client.add_reporter(&reporter);

        env.ledger().set(LedgerInfo {
            timestamp: 1000,
            ..Default::default()
        });
        client.submit_price(&reporter, &base, &quote, &1_050_000_000_i128); // 1.05

        env.ledger().set(LedgerInfo {
            timestamp: 2000,
            ..Default::default()
        });
        client.submit_price(&reporter, &base, &quote, &1_060_000_000_i128); // 1.06

        let twap = client.get_twap(&base, &quote);
        assert_eq!(twap, 1_055_000_000); // avg of 1.05 and 1.06
    }

    #[test]
    fn test_unauthorized_reporter_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let stranger = Address::generate(&env);
        let base = Address::generate(&env);
        let quote = Address::generate(&env);
        let cid = env.register_contract(None, PriceOracle);
        let client = PriceOracleClient::new(&env, &cid);
        client.initialize(&admin, &200_u32);
        // stranger not added as reporter
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.submit_price(&stranger, &base, &quote, &1_000_000_000_i128);
        }));
        assert!(result.is_err());
    }
}
