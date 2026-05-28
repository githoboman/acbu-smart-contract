use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, BytesN, Env, String};
fn generate_unique_tx_id(env: &Env, _user: &Address, _amount: i128, prefix: &str) -> String {     String::from_str(env, prefix) }
