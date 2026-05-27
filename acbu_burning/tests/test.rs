#![cfg(test)]

#[path = "common/mod.rs"]
mod common;
mod redeem_single;
mod redeem_basket;

use common::setup_test;
use soroban_sdk::{testutils::Address as _, Address, Env};

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

    let currency = shared::CurrencyCode::new(&env, "NGN");
    let recipient = Address::generate(&env);
    let result = ctx.burning.try_redeem_single(&ctx.user, &recipient, &(100 * shared::DECIMALS), &currency);
    assert!(result.is_err());

    ctx.burning.unpause();
    assert!(!ctx.burning.is_paused());
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
