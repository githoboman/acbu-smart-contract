#![cfg(test)]

#[path = "common/mod.rs"]
mod common;
mod redeem_single;
mod redeem_basket;

use common::setup_test;
use soroban_sdk::{testutils::Address as _, vec, Address, Env, Vec};

use crate::{BurningContract, BurningContractClient};
use shared::{CurrencyCode, DECIMALS};

#[test]
fn test_burning_initialize_and_version() {
    let env = Env::default();
    let ctx = setup_test(&env);

    assert_eq!(ctx.burning.version(), 2);
    assert_eq!(ctx.burning.get_fee_rate(), 100);
    assert_eq!(ctx.burning.get_fee_single_redeem(), 200);
}

#[test]
fn test_pause_unpause() {
    let env = Env::default();
    let ctx = setup_test(&env);

    ctx.burning.pause();
    assert!(ctx.burning.is_paused());

    let currency = CurrencyCode::new(&env, "NGN");
    let recipient = Address::generate(&env);

    let result = ctx.burning.try_redeem_single(
        &ctx.user,
        &recipient,
        &(100 * DECIMALS),
        &currency,
    );

    assert!(result.is_err());

    ctx.burning.unpause();
    assert!(!ctx.burning.is_paused());
}

#[test]
#[should_panic]
fn test_redeem_basket_requires_vault_allowance() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    let oracle = env.register_contract(None, oracle_mock::MockOracle);
    let reserve_tracker = env.register_contract(None, oracle_mock::MockReserveTracker);

    let acbu_token = env.register_contract(None, oracle_mock::MockToken);

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

    let burn_amt = 100 * DECIMALS;

    let recipients = vec![
        &env,
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
    ];

    client.redeem_basket(&user, &recipients, &burn_amt);
}

#[test]
fn test_redeem_basket() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    let oracle = env.register_contract(None, oracle_mock::MockOracle);
    let reserve_tracker = env.register_contract(None, oracle_mock::MockReserveTracker);

    let acbu_token = env.register_contract(None, oracle_mock::MockToken);

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

    let burn_amt = 100 * DECIMALS;

    let st_sac = soroban_sdk::token::StellarAssetClient::new(&env, &stoken);
    st_sac.mint(&vault, &(1_000_000 * DECIMALS));

    let token = soroban_sdk::token::Client::new(&env, &stoken);

    token.approve(
        &vault,
        &contract_id,
        &1_000_000_000_000_000,
        &100u32,
    );

    // C-057: provide 3 distinct recipients (one per basket currency)
    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);
    let r3 = Address::generate(&env);

    let recipients = vec![&env, r1, r2, r3];

    let amounts = client.redeem_basket(&user, &recipients, &burn_amt);

    assert_eq!(amounts.len(), 3);

    let mut total_out = 0i128;

    for amount in amounts.iter() {
        total_out += amount;
    }

    // With DECIMALS matching and weight sum = 10000,
    // out should be burn_amt - fee
    let expected_fee = (burn_amt * 100) / 10_000;

    assert_eq!(total_out + expected_fee, burn_amt);
}

#[test]
fn test_redeem_basket_rejects_empty_recipients() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    let oracle = env.register_contract(None, oracle_mock::MockOracle);
    let reserve_tracker = env.register_contract(None, oracle_mock::MockReserveTracker);

    let acbu_token = env.register_contract(None, oracle_mock::MockToken);

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

    let empty: Vec<Address> = Vec::new(&env);

    let result =
        client.try_redeem_basket(&user, &empty, &(100 * DECIMALS));

    assert!(result.is_err());
}

#[test]
fn test_set_fee_rates() {
    let env = Env::default();
    let ctx = setup_test(&env);

    ctx.burning.set_fee_rate(&50);
    assert_eq!(ctx.burning.get_fee_rate(), 50);

    ctx.burning.set_fee_single_redeem(&150);
    assert_eq!(ctx.burning.get_fee_single_redeem(), 150);
}

#[test]
fn test_update_oracle_by_admin_burning() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);

    let oracle = env.register_contract(None, oracle_mock::MockOracle);
    let reserve_tracker =
        env.register_contract(None, oracle_mock::MockReserveTracker);

    let acbu_token = env.register_contract(None, oracle_mock::MockToken);

    let vault = admin.clone();
    let withdrawal_processor = Address::generate(&env);

    let contract_id = env.register_contract(None, BurningContract);

    let client = BurningContractClient::new(&env, &contract_id);

    client.initialize(
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token,
        &withdrawal_processor,
        &vault,
        &100,
        &150,
    );

    let new_oracle = Address::generate(&env);

    client.update_oracle(&new_oracle);
}

#[test]
fn test_update_reserve_tracker_by_admin_burning() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);

    let oracle = env.register_contract(None, oracle_mock::MockOracle);
    let reserve_tracker =
        env.register_contract(None, oracle_mock::MockReserveTracker);

    let acbu_token = env.register_contract(None, oracle_mock::MockToken);

    let vault = admin.clone();
    let withdrawal_processor = Address::generate(&env);

    let contract_id = env.register_contract(None, BurningContract);

    let client = BurningContractClient::new(&env, &contract_id);

    client.initialize(
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token,
        &withdrawal_processor,
        &vault,
        &100,
        &150,
    );

    let new_rt = Address::generate(&env);

    client.update_reserve_tracker(&new_rt);
}

#[test]
fn test_update_acbu_token_by_admin_burning() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);

    let oracle = env.register_contract(None, oracle_mock::MockOracle);
    let reserve_tracker =
        env.register_contract(None, oracle_mock::MockReserveTracker);

    let acbu_token = env.register_contract(None, oracle_mock::MockToken);

    let vault = admin.clone();
    let withdrawal_processor = Address::generate(&env);

    let contract_id = env.register_contract(None, BurningContract);

    let client = BurningContractClient::new(&env, &contract_id);

    client.initialize(
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token,
        &withdrawal_processor,
        &vault,
        &100,
        &150,
    );

    let new_token = Address::generate(&env);

    client.update_acbu_token(&new_token);
}

#[test]
fn test_update_vault_by_admin_burning() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);

    let oracle = env.register_contract(None, oracle_mock::MockOracle);
    let reserve_tracker =
        env.register_contract(None, oracle_mock::MockReserveTracker);

    let acbu_token = env.register_contract(None, oracle_mock::MockToken);

    let vault = admin.clone();
    let withdrawal_processor = Address::generate(&env);

    let contract_id = env.register_contract(None, BurningContract);

    let client = BurningContractClient::new(&env, &contract_id);

    client.initialize(
        &admin,
        &oracle,
        &reserve_tracker,
        &acbu_token,
        &withdrawal_processor,
        &vault,
        &100,
        &150,
    );

    let new_vault = Address::generate(&env);

    client.update_vault(&new_vault);
}