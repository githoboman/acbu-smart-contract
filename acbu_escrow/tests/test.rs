#![cfg(test)]

use acbu_escrow::{Escrow, EscrowClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup(env: &Env) -> (EscrowClient<'_>, Address, Address) {
    env.mock_all_auths();

    let admin = Address::generate(env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    let contract_id = env.register_contract(None, Escrow);
    let client = EscrowClient::new(env, &contract_id);
    client.initialize(&admin, &acbu_token);

    (client, admin, acbu_token)
}

fn mint(env: &Env, _admin: &Address, token: &Address, to: &Address, amount: i128) {
    soroban_sdk::token::StellarAssetClient::new(env, token).mint(to, &amount);
}
use acbu_escrow::{Escrow, EscrowClient, EscrowError};
use soroban_sdk::{
    testutils::{Address as _, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal,
};

#[test]
fn create_locks_payer_funds() {
    let env = Env::default();
    let (client, admin, token) = setup(&env);
    let payer = Address::generate(&env);
    let payee = Address::generate(&env);
    let amount = 1_000i128;

    mint(&env, &admin, &token, &payer, amount);
    client.create(&payer, &payee, &amount, &1);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
    env.mock_all_auths();
    token_admin.mint(&payer, &amount);

    let contract_id = env.register_contract(None, Escrow);
    let client = EscrowClient::new(&env, &contract_id);

    client.initialize(&admin, &acbu_token);
    client.create(&payer, &payee, &amount, &escrow_id);

    let token_client = soroban_sdk::token::Client::new(&env, &token);
    assert_eq!(token_client.balance(&payer), 0);
    assert_eq!(token_client.balance(&client.address), amount);
}

#[test]
fn release_pays_payee_and_removes_escrow() {
    let env = Env::default();
    let (client, admin, token) = setup(&env);
    let payer = Address::generate(&env);
    let payee = Address::generate(&env);
    let amount = 1_500i128;
    let escrow_id = 7u64;
    let escrow_id = 99u64;
    let amount = 12_500_000i128;

    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin = soroban_sdk::token::StellarAssetClient::new(&env, &acbu_token);
    env.mock_all_auths();
    token_admin.mint(&payer, &amount);

    mint(&env, &admin, &token, &payer, amount);
    client.create(&payer, &payee, &amount, &escrow_id);
    client.release(&escrow_id, &payer);

    let token_client = soroban_sdk::token::Client::new(&env, &token);
    assert_eq!(token_client.balance(&payee), amount);
    assert!(client.try_release(&escrow_id, &payer).is_err());
}

#[test]
fn refund_returns_funds_to_payer_and_removes_escrow() {
    let env = Env::default();
    let (client, admin, token) = setup(&env);
    let payer = Address::generate(&env);
    let payee = Address::generate(&env);
    let amount = 2_000i128;
    let escrow_id = 9u64;
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    let contract_id = env.register_contract(None, Escrow);
    let client = EscrowClient::new(&env, &contract_id);

    mint(&env, &admin, &token, &payer, amount);
    client.create(&payer, &payee, &amount, &escrow_id);
    client.refund(&escrow_id, &payer);

    let token_client = soroban_sdk::token::Client::new(&env, &token);
    assert_eq!(token_client.balance(&payer), amount);
    assert!(client.try_refund(&escrow_id, &payer).is_err());
    let result = client.try_release(&1u64, &payer);
    assert_eq!(result, Err(Ok(EscrowError::EscrowNotFound)));
}

#[test]
fn different_payers_can_reuse_same_escrow_id_without_collision() {
    let env = Env::default();
    let (client, admin, token) = setup(&env);
    let payer_a = Address::generate(&env);
    let payer_b = Address::generate(&env);
    let payee = Address::generate(&env);
    let escrow_id = 42u64;

    mint(&env, &admin, &token, &payer_a, 700);
    mint(&env, &admin, &token, &payer_b, 300);
    let admin = Address::generate(&env);
    let payer = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    client.create(&payer_a, &payee, &700, &escrow_id);
    client.create(&payer_b, &payee, &300, &escrow_id);

    client.release(&escrow_id, &payer_a);
    client.release(&escrow_id, &payer_b);

    let token_client = soroban_sdk::token::Client::new(&env, &token);
    assert_eq!(token_client.balance(&payee), 1_000);
    let result = client.try_refund(&1u64, &payer);
    assert_eq!(result, Err(Ok(EscrowError::EscrowNotFound)));
}

#[test]
fn same_payer_same_escrow_id_is_rejected_until_released() {
    let env = Env::default();
    let (client, admin, token) = setup(&env);
    let payer = Address::generate(&env);
    let payee = Address::generate(&env);
    let escrow_id = 11u64;

    mint(&env, &admin, &token, &payer, 1_000);
    client.create(&payer, &payee, &400, &escrow_id);
    assert!(client.try_create(&payer, &payee, &100, &escrow_id).is_err());

    client.release(&escrow_id, &payer);
    client.create(&payer, &payee, &100, &escrow_id);
    let result = client.try_pause();
    assert_eq!(result, Err(Ok(EscrowError::UninitializedAdmin)));
}

#[test]
fn test_update_acbu_token_by_admin_escrow() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let acbu_token = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    let contract_id = env.register_contract(None, Escrow);
    let client = EscrowClient::new(&env, &contract_id);
    client.initialize(&admin, &acbu_token);

    let new_token = Address::generate(&env);
    client.update_acbu_token(&new_token);
}
