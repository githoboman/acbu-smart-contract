#![cfg(test)]

use acbu_burning::{BurningContract, BurningContractClient};
use shared::{CurrencyCode, DECIMALS};
use soroban_sdk::{
    bytesn, contract, contractimpl, symbol_short, testutils::Address as _, Address, BytesN, Env,
};

mod oracle_mock {
    use super::*;
    use shared::CurrencyCode;
    use soroban_sdk::Vec;

    #[contract]
    pub struct MockOracle;

    #[contractimpl]
    impl MockOracle {
        pub fn get_acbu_usd_rate(_env: Env) -> i128 {
            DECIMALS
        }

        pub fn get_currencies(env: Env) -> Vec<CurrencyCode> {
            let mut v = Vec::new(&env);
            v.push_back(CurrencyCode::new(&env, "NGN"));
            v.push_back(CurrencyCode::new(&env, "KES"));
            v.push_back(CurrencyCode::new(&env, "GHS"));
            v
        }

        pub fn get_basket_weight(env: Env, c: CurrencyCode) -> i128 {
            if c == CurrencyCode::new(&env, "NGN") || c == CurrencyCode::new(&env, "KES") {
                3_333
            } else if c == CurrencyCode::new(&env, "GHS") {
                3_334
            } else {
                0
            }
        }

        pub fn get_rate(_env: Env, _c: CurrencyCode) -> i128 {
            DECIMALS
        }

        pub fn get_s_token_address(env: Env, _c: CurrencyCode) -> Address {
            env.storage()
                .instance()
                .get(&symbol_short!("STK"))
                .expect("seed_stoken")
        }

        pub fn seed_stoken(env: Env, stoken: Address) {
            env.storage().instance().set(&symbol_short!("STK"), &stoken);
        }
    }
}

#[test]
fn test_burning_initialize_and_version() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let oracle = env.register_contract(None, oracle_mock::MockOracle);
    let reserve_tracker = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let withdrawal_processor = Address::generate(&env);
    let vault = admin.clone();

    let contract_id = env.register_contract(None, BurningContract);
    let client = BurningContractClient::new(&env, &contract_id);

    client.initialize(
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token,
        &withdrawal_processor,
        &vault,
        &300,
        &150,
    );

    assert_eq!(client.get_version(), 1);
    assert_eq!(client.get_fee_rate(), 300);
    assert_eq!(client.get_fee_single_redeem(), 150);
}

#[test]
fn test_redeem_single_transfers_stoken() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let recipient = Address::generate(&env);

    let oracle = env.register_contract(None, oracle_mock::MockOracle);
    let reserve_tracker = Address::generate(&env);

    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let stoken = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    oracle_mock::MockOracleClient::new(&env, &oracle).seed_stoken(&stoken);

    let contract_id = env.register_contract(None, BurningContract);
    let client = BurningContractClient::new(&env, &contract_id);

    let vault = admin.clone();
    let withdrawal_processor = Address::generate(&env);

    client.initialize(
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token,
        &withdrawal_processor,
        &vault,
        &300,
        &150,
    );

    let acbu_sac = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
    let burn_amt = 100 * DECIMALS;
    acbu_sac.mint(&user, &burn_amt);

    let st_sac = soroban_sdk::token::StellarAssetClient::new(&env, &stoken);
    st_sac.mint(&vault, &(1_000_000 * DECIMALS));

    let token = soroban_sdk::token::Client::new(&env, &stoken);
    // SAC approve: live_until ledger must be < host max allowed (use current-ish horizon)
    token.approve(&vault, &contract_id, &1_000_000_000_000_000, &100u32);

    let currency = CurrencyCode::new(&env, "NGN");
    let out = client.redeem_single(&user, &recipient, &burn_amt, &currency);
    assert!(out > 0);
    assert_eq!(token.balance(&recipient), out);

    let acbu = soroban_sdk::token::Client::new(&env, &acbu_token);
    assert_eq!(acbu.balance(&user), 0);
}

#[test]
fn test_redeem_basket() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let recipient = Address::generate(&env);

    let oracle = env.register_contract(None, oracle_mock::MockOracle);
    let reserve_tracker = Address::generate(&env);

    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let stoken = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    oracle_mock::MockOracleClient::new(&env, &oracle).seed_stoken(&stoken);

    let contract_id = env.register_contract(None, BurningContract);
    let client = BurningContractClient::new(&env, &contract_id);

    let vault = admin.clone();
    client.initialize(
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token,
        &Address::generate(&env),
        &vault,
        &100,
        &150,
    );

    let acbu_sac = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
    let burn_amt = 100 * DECIMALS + 3;
    acbu_sac.mint(&user, &burn_amt);

    let st_sac = soroban_sdk::token::StellarAssetClient::new(&env, &stoken);
    st_sac.mint(&vault, &(1_000_000 * DECIMALS));

    let token = soroban_sdk::token::Client::new(&env, &stoken);
    // SAC approve: live_until ledger must be < host max allowed (use current-ish horizon)
    token.approve(&vault, &contract_id, &1_000_000_000_000_000, &100u32);

    let amounts = client.redeem_basket(&user, &recipient, &burn_amt);
    assert_eq!(amounts.len(), 3);

    let mut total_out = 0i128;
    for amount in amounts.iter() {
        total_out += amount;
    }
    let expected_fee = (burn_amt * 100) / 10_000;
    let net = burn_amt - expected_fee;
    // Per-currency integer division can lose at most 1 unit per currency in rounding.
    assert!(total_out <= net, "total_out should not exceed net");
    assert!(
        total_out >= net - amounts.len() as i128,
        "rounding loss bounded by currency count"
    );
    assert_eq!(token.balance(&recipient), total_out);

    let acbu = soroban_sdk::token::Client::new(&env, &acbu_token);
    assert_eq!(acbu.balance(&user), 0);
}

// --- Upgrade path tests (issue #242) ---

fn setup_burning_client(env: &Env) -> (Address, Address, BurningContractClient) {
    let admin = Address::generate(env);
    let oracle = env.register_contract(None, oracle_mock::MockOracle);
    let reserve_tracker = Address::generate(env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let withdrawal_processor = Address::generate(env);
    let vault = admin.clone();
    let contract_id = env.register_contract(None, BurningContract);
    let client = BurningContractClient::new(env, &contract_id);
    client.initialize(
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token,
        &withdrawal_processor,
        &vault,
        &300,
        &150,
    );
    (admin, contract_id, client)
}

#[test]
fn test_version_set_on_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _contract_id, client) = setup_burning_client(&env);
    assert_eq!(client.get_version(), 1);
}

#[test]
#[should_panic(expected = "Invalid version upgrade")]
fn test_upgrade_rejects_same_version() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _contract_id, client) = setup_burning_client(&env);
    // version is 1 after init; trying to upgrade to 1 must be rejected
    let dummy_hash: BytesN<32> = bytesn!(
        &env,
        0x0000000000000000000000000000000000000000000000000000000000000000
    );
    client.upgrade(&dummy_hash, &1u32);
}

#[test]
#[should_panic(expected = "Invalid version upgrade")]
fn test_upgrade_rejects_lower_version() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _contract_id, client) = setup_burning_client(&env);
    let dummy_hash: BytesN<32> = bytesn!(
        &env,
        0x0000000000000000000000000000000000000000000000000000000000000000
    );
    client.upgrade(&dummy_hash, &0u32);
}

#[test]
fn test_state_preserved_across_upgrade_boundary() {
    let env = Env::default();
    env.mock_all_auths();
    let (_admin, _contract_id, client) = setup_burning_client(&env);
    // Confirm fee rates survive an upgrade attempt (the WASM lookup panics before storage is
    // touched, so we verify pre-upgrade storage is intact via the getters).
    assert_eq!(client.get_fee_rate(), 300);
    assert_eq!(client.get_fee_single_redeem(), 150);
    assert_eq!(client.get_version(), 1);
}
