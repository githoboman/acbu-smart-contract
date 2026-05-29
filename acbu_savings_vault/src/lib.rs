#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env,
    Symbol, Vec,
};

use shared::{calculate_fee, DataKey as SharedDataKey, reentrancy_guard, BASIS_POINTS, CONTRACT_VERSION};

mod shared {
    pub use shared::*;
}

// ---------------------------------------------------------------------------
// Error codes — every contract_error code is documented here.
// ---------------------------------------------------------------------------
#[soroban_sdk::contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    Paused = 1001,
    InvalidAmount = 1002,
    NoDeposit = 1003,
    AccountingError = 1004,
    Overflow = 1005,
    InsufficientUnlocked = 1006,
    InvalidTerm = 1007,
    NotInitialized = 1008,
    NoAdmin = 1009,
    AlreadyInitialized = 1010,
    InvalidFeeRate = 1011,
    InvalidYieldRate = 1012,
    NoFeeRate = 1013,
    NoYieldRate = 1014,
    ZeroNetDeposit = 1015,
    InvalidVersion = 1016,
    TimelockNotElapsed = 1017,
    NoPendingUpgrade = 1018,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
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

const DATA_KEY: DataKey = DataKey {
    admin: symbol_short!("ADMIN"),
    acbu_token: symbol_short!("ACBU_TKN"),
    fee_rate: symbol_short!("FEE_RATE"),
    yield_rate: symbol_short!("YLD_RATE"),
    paused: symbol_short!("PAUSED"),
    pending_upgrade_wasm: symbol_short!("PU_WASM"),
    pending_upgrade_version: symbol_short!("PU_VER"),
    pending_upgrade_eligible_at: symbol_short!("PU_ETA"),
};

const DEPOSIT_KEY: Symbol = symbol_short!("DEPOSITS");
const SECONDS_PER_YEAR: i128 = 31_536_000;
const UPGRADE_TIMELOCK_SECONDS: u64 = 86_400;
// CONTRACT_VERSION is imported from shared

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------
#[contracttype]
#[derive(Clone, Debug)]
pub struct DepositLot {
    pub amount: i128,
    pub timestamp: u64,
    pub term_seconds: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct DepositEvent {
    pub user: Address,
    pub gross_amount: i128,
    pub fee_amount: i128,
    pub net_amount: i128,
    pub term_seconds: u64,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct WithdrawEvent {
    pub user: Address,
    pub amount: i128,
    pub fee_amount: i128,
    pub yield_amount: i128,
    pub timestamp: u64,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------
#[contract]
pub struct SavingsVault;

#[contractimpl]
impl SavingsVault {
    // -----------------------------------------------------------------------
    // Internal helpers — read required state, return typed errors on miss
    // -----------------------------------------------------------------------

    fn load_admin(env: &Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DATA_KEY.admin)
            .ok_or(Error::NoAdmin)
    }

    fn load_acbu_token(env: &Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DATA_KEY.acbu_token)
            .ok_or(Error::NotInitialized)
    }

    fn load_fee_rate(env: &Env) -> Result<i128, Error> {
        env.storage()
            .instance()
            .get(&DATA_KEY.fee_rate)
            .ok_or(Error::NoFeeRate)
    }

    fn load_yield_rate(env: &Env) -> Result<i128, Error> {
        env.storage()
            .instance()
            .get(&DATA_KEY.yield_rate)
            .ok_or(Error::NoYieldRate)
    }

    fn is_paused(env: &Env) -> bool {
        env.storage()
            .instance()
            .get(&DATA_KEY.paused)
            .unwrap_or(false)
    }

    // -----------------------------------------------------------------------
    // Public logic
    // -----------------------------------------------------------------------

    pub fn initialize(
        env: Env,
        admin: Address,
        acbu_token: Address,
        fee_rate_bps: i128,
        yield_rate_bps: i128,
    ) {
        if env.storage().instance().has(&DATA_KEY.admin) {
            env.panic_with_error(Error::AlreadyInitialized);
        }
        if !(0..=BASIS_POINTS).contains(&fee_rate_bps) {
            env.panic_with_error(Error::InvalidFeeRate);
        }
        if !(0..=BASIS_POINTS).contains(&yield_rate_bps) {
            env.panic_with_error(Error::InvalidYieldRate);
        }
        env.storage().instance().set(&DATA_KEY.admin, &admin);
        env.storage()
            .instance()
            .set(&DATA_KEY.acbu_token, &acbu_token);
        env.storage()
            .instance()
            .set(&DATA_KEY.fee_rate, &fee_rate_bps);
        env.storage()
            .instance()
            .set(&DATA_KEY.yield_rate, &yield_rate_bps);
        env.storage().instance().set(&DATA_KEY.paused, &false);
        env.storage()
            .instance()
            .set(&SharedDataKey::Version, &CONTRACT_VERSION);
    }

    /// Deposit (lock) ACBU for a term. User transfers ACBU to this contract.
    pub fn deposit(env: Env, user: Address, amount: i128, term_seconds: u64) -> i128 {
        // Re-entrancy guard
        reentrancy_guard::acquire_guard(&env);

        user.require_auth();

        if Self::is_paused(&env) {
            env.panic_with_error(Error::Paused);
        }
        if amount <= 0 {
            env.panic_with_error(Error::InvalidAmount);
        }
        if term_seconds == 0 {
            env.panic_with_error(Error::InvalidTerm);
        }

        // Load fee_rate and calculate fee before transfer.
        let fee_rate = Self::load_fee_rate(&env).unwrap_or_else(|e| env.panic_with_error(e));
        let fee_amount = calculate_fee(amount, fee_rate);
        let net_amount = amount
            .checked_sub(fee_amount)
            .unwrap_or_else(|| env.panic_with_error(Error::Overflow));

        // Guard against fee consuming entire deposit.
        if net_amount <= 0 {
            env.panic_with_error(Error::ZeroNetDeposit);
        }

        let acbu = Self::load_acbu_token(&env).unwrap_or_else(|e| env.panic_with_error(e));
        let admin = Self::load_admin(&env).unwrap_or_else(|e| env.panic_with_error(e));
        let token = soroban_sdk::token::Client::new(&env, &acbu);
        let vault_addr = env.current_contract_address();

        // CEI: record the deposit lot before the external token transfers so any
        // token-level callback sees the new deposit as already committed.
        let key = (DEPOSIT_KEY, user.clone(), term_seconds);
        let mut lots: Vec<DepositLot> = env
            .storage()
            .temporary()
            .get(&key)
            .unwrap_or(Vec::new(&env));

        lots.push_back(DepositLot {
            // Store net_amount instead of gross amount.
            amount: net_amount,
            timestamp: env.ledger().timestamp(),
            term_seconds,
        });

        env.storage().temporary().set(&key, &lots);

        // Transfer the net amount to the vault and the fee to the admin.
        token.transfer(&user, &vault_addr, &net_amount);
        if fee_amount > 0 {
            token.transfer(&user, &admin, &fee_amount);
        }

        env.events().publish(
            (symbol_short!("Deposit"), user.clone()),
            DepositEvent {
                user,
                gross_amount: amount,
                fee_amount,
                net_amount,
                term_seconds,
                timestamp: env.ledger().timestamp(),
            },
        );

        // Release re-entrancy guard
        reentrancy_guard::release_guard(&env);

        net_amount
    }

    /// Withdraw unlocked ACBU + yield for a specific term.
    pub fn withdraw(env: Env, user: Address, term_seconds: u64, amount: i128) -> i128 {
        // Re-entrancy guard
        reentrancy_guard::acquire_guard(&env);

        user.require_auth();

        if Self::is_paused(&env) {
            env.panic_with_error(Error::Paused);
        }
        if amount <= 0 {
            env.panic_with_error(Error::InvalidAmount);
        }

        let key = (DEPOSIT_KEY, user.clone(), term_seconds);
        let lots: Vec<DepositLot> = env
            .storage()
            .temporary()
            .get(&key)
            .unwrap_or_else(|| env.panic_with_error(Error::NoDeposit));

        let now = env.ledger().timestamp();

        // Compute total unlocked balance.
        let mut unlocked_balance = 0i128;
        for lot in lots.iter() {
            if now >= lot.timestamp.saturating_add(lot.term_seconds) {
                unlocked_balance = unlocked_balance
                    .checked_add(lot.amount)
                    .unwrap_or_else(|| env.panic_with_error(Error::Overflow));
            }
        }

        if amount > unlocked_balance {
            env.panic_with_error(Error::InsufficientUnlocked);
        }

        // Fee is not charged on withdraw — only yield is added.
        let yield_rate = Self::load_yield_rate(&env).unwrap_or_else(|e| env.panic_with_error(e));

        let mut amount_left = amount;
        let mut yield_amount = 0i128;
        let mut updated_lots = Vec::new(&env);

        for lot in lots.iter() {
            if amount_left == 0 || now < lot.timestamp.saturating_add(lot.term_seconds) {
                updated_lots.push_back(lot);
                continue;
            }

            if lot.amount <= amount_left {
                amount_left = amount_left
                    .checked_sub(lot.amount)
                    .unwrap_or_else(|| env.panic_with_error(Error::AccountingError));
                let elapsed = now.saturating_sub(lot.timestamp);
                let lot_yield = Self::calculate_yield(lot.amount, yield_rate, elapsed)
                    .unwrap_or_else(|e| env.panic_with_error(e));
                yield_amount = yield_amount
                    .checked_add(lot_yield)
                    .unwrap_or_else(|| env.panic_with_error(Error::Overflow));
            } else {
                let consumed = amount_left;
                let remaining = lot
                    .amount
                    .checked_sub(consumed)
                    .unwrap_or_else(|| env.panic_with_error(Error::AccountingError));
                let elapsed = now.saturating_sub(lot.timestamp);
                let lot_yield = Self::calculate_yield(consumed, yield_rate, elapsed)
                    .unwrap_or_else(|e| env.panic_with_error(e));
                yield_amount = yield_amount
                    .checked_add(lot_yield)
                    .unwrap_or_else(|| env.panic_with_error(Error::Overflow));
                updated_lots.push_back(DepositLot {
                    amount: remaining,
                    timestamp: lot.timestamp,
                    term_seconds: lot.term_seconds,
                });
                amount_left = 0;
            }
        }

        if amount_left > 0 {
            env.panic_with_error(Error::AccountingError);
        }

        if updated_lots.is_empty() {
            env.storage().temporary().remove(&key);
        } else {
            env.storage().temporary().set(&key, &updated_lots);
        }

        let payout_amount = amount
            .checked_add(yield_amount)
            .unwrap_or_else(|| env.panic_with_error(Error::Overflow));

        // Single storage read for the token — reuse the client for both transfers.
        let acbu = Self::load_acbu_token(&env).unwrap_or_else(|e| env.panic_with_error(e));
        let token = soroban_sdk::token::Client::new(&env, &acbu);
        let vault_addr = env.current_contract_address();

        // 1. Return the principal from this contract to user.
        token.transfer(&vault_addr, &user, &amount);
        // 2. Transfer the yield (assumes contract holds sufficient ACBU balance).
        if yield_amount > 0 {
            token.transfer(&vault_addr, &user, &yield_amount);
        }

        env.events().publish(
            (symbol_short!("Withdraw"), user.clone()),
            WithdrawEvent {
                user,
                amount,
                fee_amount: 0, // No fee on withdraw
                yield_amount,
                timestamp: now,
            },
        );

        // Release re-entrancy guard
        reentrancy_guard::release_guard(&env);

        payout_amount
    }

    pub fn get_balance(env: Env, user: Address, term_seconds: u64) -> i128 {
        let key = (DEPOSIT_KEY, user, term_seconds);
        let lots: Vec<DepositLot> = env
            .storage()
            .temporary()
            .get(&key)
            .unwrap_or(Vec::new(&env));
        Self::sum_lots(&lots)
    }

    pub fn get_pending_yield(env: Env, user: Address, term_seconds: u64) -> i128 {
        let key = (DEPOSIT_KEY, user, term_seconds);
        let lots: Vec<DepositLot> = env
            .storage()
            .temporary()
            .get(&key)
            .unwrap_or_else(|| env.panic_with_error(Error::NoDeposit));

        let yield_rate = Self::load_yield_rate(&env).unwrap_or_else(|e| env.panic_with_error(e));
        let now = env.ledger().timestamp();
        let mut yield_amount = 0i128;

        for lot in lots.iter() {
            let unlocked = now >= lot.timestamp.saturating_add(lot.term_seconds);
            if !unlocked {
                continue;
            }
            let elapsed = now.saturating_sub(lot.timestamp);
            let lot_yield = Self::calculate_yield(lot.amount, yield_rate, elapsed)
                .unwrap_or_else(|e| env.panic_with_error(e));
            yield_amount = yield_amount
                .checked_add(lot_yield)
                .unwrap_or_else(|| env.panic_with_error(Error::Overflow));
        }

        yield_amount
    }

    pub fn pause(env: Env) {
        let admin = Self::load_admin(&env).unwrap_or_else(|e| env.panic_with_error(e));
        admin.require_auth();
        env.storage().instance().set(&DATA_KEY.paused, &true);
    }

    pub fn unpause(env: Env) {
        let admin = Self::load_admin(&env).unwrap_or_else(|e| env.panic_with_error(e));
        admin.require_auth();
        env.storage().instance().set(&DATA_KEY.paused, &false);
    }

    pub fn get_version(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&SharedDataKey::Version)
            .unwrap_or(0)
    }

    pub fn update_acbu_token(env: Env, new_acbu_token: Address) {
        let admin = Self::load_admin(&env).unwrap_or_else(|e| env.panic_with_error(e));
        admin.require_auth();
        env.storage()
            .instance()
            .set(&DATA_KEY.acbu_token, &new_acbu_token);
    }

    pub fn propose_upgrade(env: Env, new_wasm_hash: BytesN<32>, new_version: u32) {
        let admin = Self::load_admin(&env).unwrap_or_else(|e| env.panic_with_error(e));
        admin.require_auth();
        let current_version: u32 = env
            .storage()
            .instance()
            .get(&SharedDataKey::Version)
            .unwrap_or(0);
        if new_version <= current_version {
            env.panic_with_error(Error::InvalidVersion);
        }
        let eligible_at = env.ledger().timestamp() + UPGRADE_TIMELOCK_SECONDS;
        env.storage()
            .instance()
            .set(&DATA_KEY.pending_upgrade_wasm, &new_wasm_hash);
        env.storage()
            .instance()
            .set(&DATA_KEY.pending_upgrade_version, &new_version);
        env.storage()
            .instance()
            .set(&DATA_KEY.pending_upgrade_eligible_at, &eligible_at);
    }

    pub fn execute_upgrade(env: Env) {
        let admin = Self::load_admin(&env).unwrap_or_else(|e| env.panic_with_error(e));
        admin.require_auth();
        let wasm_hash: BytesN<32> = env
            .storage()
            .instance()
            .get(&DATA_KEY.pending_upgrade_wasm)
            .unwrap_or_else(|| env.panic_with_error(Error::NoPendingUpgrade));
        let new_version: u32 = env
            .storage()
            .instance()
            .get(&DATA_KEY.pending_upgrade_version)
            .unwrap_or_else(|| env.panic_with_error(Error::NoPendingUpgrade));
        let eligible_at: u64 = env
            .storage()
            .instance()
            .get(&DATA_KEY.pending_upgrade_eligible_at)
            .unwrap_or(u64::MAX);
        if env.ledger().timestamp() < eligible_at {
            env.panic_with_error(Error::TimelockNotElapsed);
        }
        let current_version: u32 = env
            .storage()
            .instance()
            .get(&SharedDataKey::Version)
            .unwrap_or(0);
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_upgrade_wasm);
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_upgrade_version);
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_upgrade_eligible_at);
        env.deployer().update_current_contract_wasm(wasm_hash);
        for v in current_version..new_version {
            match v {
                0 => Self::migrate_v0_to_v1(env.clone()),
                _ => {}
            }
        }
        env.storage()
            .instance()
            .set(&SharedDataKey::Version, &new_version);
    }

    pub fn cancel_upgrade(env: Env) {
        let admin = Self::load_admin(&env).unwrap_or_else(|e| env.panic_with_error(e));
        admin.require_auth();
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_upgrade_wasm);
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_upgrade_version);
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_upgrade_eligible_at);
    }

    fn migrate_v0_to_v1(_env: Env) {
        // Migration logic
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    fn sum_lots(lots: &Vec<DepositLot>) -> i128 {
        let mut total = 0i128;
        for lot in lots.iter() {
            total = total
                .checked_add(lot.amount)
                .expect("Overflow in total balance calculation");
        }
        total
    }

    fn calculate_yield(
        principal: i128,
        yield_rate_bps: i128,
        elapsed_seconds: u64,
    ) -> Result<i128, Error> {
        let elapsed_i128 = i128::from(elapsed_seconds);
        let numerator = principal
            .checked_mul(yield_rate_bps)
            .and_then(|v| v.checked_mul(elapsed_i128))
            .ok_or(Error::Overflow)?;
        Ok(numerator / (BASIS_POINTS * SECONDS_PER_YEAR))
    }
}