#![no_std]
use soroban_sdk::{contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Vec, Symbol};

// --- Definitions (These were missing, now included here) ---
#[contracttype]
pub struct DataKey {
    pub admin: Symbol,
    pub acbu_token: Symbol,
    pub fee_rate: Symbol,
    pub yield_rate: Symbol,
    pub paused: Symbol,
    pub pending_upgrade_wasm: Symbol,
    pub pending_upgrade_version: Symbol,
    pub pending_upgrade_eligible_at: Symbol,
}

pub const DATA_KEY: DataKey = DataKey {
    admin: symbol_short!("admin"),
    acbu_token: symbol_short!("token"),
    fee_rate: symbol_short!("fee"),
    yield_rate: symbol_short!("yield"),
    paused: symbol_short!("paused"),
    pending_upgrade_wasm: symbol_short!("upg_wasm"),
    pending_upgrade_version: symbol_short!("upg_ver"),
    pending_upgrade_eligible_at: symbol_short!("upg_time"),
};

#[contracttype]
pub enum SharedDataKey { Version }

#[contracttype]
#[derive(Clone)]
pub struct DepositLot { pub amount: i128 }

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NoAdmin = 2,
    NotInitialized = 3,
    NoFeeRate = 4,
    NoYieldRate = 5,
    InvalidVersion = 6,
    NoPendingUpgrade = 7,
    TimelockNotElapsed = 8,
}

// --- Contract Implementation ---
const CONTRACT_VERSION: u32 = 1;
const UPGRADE_TIMELOCK_SECONDS: u64 = 86400;
const DEPOSIT_KEY: Symbol = symbol_short!("DEPOSIT");

#[contract]
pub struct SavingsVault;

#[contractimpl]
impl SavingsVault {
    pub fn initialize(env: Env, admin: Address, acbu_token: Address, fee_rate_bps: i128, yield_rate_bps: i128) {
        if env.storage().instance().has(&DATA_KEY.admin) { env.panic_with_error(Error::AlreadyInitialized); }
        env.storage().instance().set(&DATA_KEY.admin, &admin);
        env.storage().instance().set(&DATA_KEY.acbu_token, &acbu_token);
        env.storage().instance().set(&DATA_KEY.fee_rate, &fee_rate_bps);
        env.storage().instance().set(&DATA_KEY.yield_rate, &yield_rate_bps);
        env.storage().instance().set(&SharedDataKey::Version, &CONTRACT_VERSION);
    }

    pub fn get_balance(env: Env, user: Address, term_seconds: u64) -> i128 {
        let key = (DEPOSIT_KEY, user, term_seconds);
        let lots: Vec<DepositLot> = env.storage().temporary().get(&key).unwrap_or_else(|| Vec::new(&env));
        Self::sum_lots(&lots)
    }

    pub fn propose_upgrade(env: Env, new_wasm_hash: BytesN<32>, new_version: u32) {
        let admin = Self::load_admin(&env).unwrap_or_else(|_| env.panic_with_error(Error::NoAdmin));
        admin.require_auth();
        let current_version: u32 = env.storage().instance().get(&SharedDataKey::Version).unwrap_or(0);
        if new_version <= current_version { env.panic_with_error(Error::InvalidVersion); }
        
        env.storage().instance().set(&DATA_KEY.pending_upgrade_wasm, &new_wasm_hash);
        env.storage().instance().set(&DATA_KEY.pending_upgrade_version, &new_version);
        env.storage().instance().set(&DATA_KEY.pending_upgrade_eligible_at, &(env.ledger().timestamp() + UPGRADE_TIMELOCK_SECONDS));
    }

    pub fn execute_upgrade(env: Env) {
        let admin = Self::load_admin(&env).unwrap_or_else(|_| env.panic_with_error(Error::NoAdmin));
        admin.require_auth();
        let wasm_hash: BytesN<32> = env.storage().instance().get(&DATA_KEY.pending_upgrade_wasm).unwrap_or_else(|| env.panic_with_error(Error::NoPendingUpgrade));
        let new_version: u32 = env.storage().instance().get(&DATA_KEY.pending_upgrade_version).unwrap_or(0);
        
        env.deployer().update_current_contract_wasm(wasm_hash);
        env.storage().instance().set(&SharedDataKey::Version, &new_version);
        env.storage().instance().remove(&DATA_KEY.pending_upgrade_wasm);
        env.storage().instance().remove(&DATA_KEY.pending_upgrade_version);
        env.storage().instance().remove(&DATA_KEY.pending_upgrade_eligible_at);
    }

    fn load_admin(env: &Env) -> Result<Address, Error> {
        env.storage().instance().get(&DATA_KEY.admin).ok_or(Error::NoAdmin)
    }

    fn sum_lots(lots: &Vec<DepositLot>) -> i128 {
        lots.iter().fold(0i128, |acc, lot| acc + lot.amount)
    }
}