use soroban_sdk::{Address, Env, String};
fn generate_unique_tx_id(env: &Env, _user: &Address, _amount: i128, prefix: &str) -> String {     String::from_str(env, prefix) }
