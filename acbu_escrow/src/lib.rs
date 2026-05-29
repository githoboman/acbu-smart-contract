#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, Symbol,
};

use shared::{DataKey as SharedDataKey, CONTRACT_VERSION, reentrancy_guard};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum EscrowError {
    Paused = 3001,
    InvalidAmount = 3002,
    EscrowNotFound = 3003,
    PayerMismatch = 3004,
    EscrowExists = 3005,
    UninitializedAdmin = 3006,
    UninitializedAcBuToken = 3007,
    AlreadyInitialized = 3008,
    TimelockNotElapsed = 3009,
    NoPendingUpgrade = 3010,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EscrowDataKey {
    pub admin: Symbol,
    pub acbu_token: Symbol,
    pub paused: Symbol,
    pub pending_upgrade: Symbol,
    pub pending_upgrade_eligible_at: Symbol,
}

const DATA_KEY: EscrowDataKey = EscrowDataKey {
    admin: symbol_short!("ADMIN"),
    acbu_token: symbol_short!("ACBU_TKN"),
    paused: symbol_short!("PAUSED"),
    pending_upgrade: symbol_short!("PEND_UPG"),
    pending_upgrade_eligible_at: symbol_short!("PU_ETA"),
};

const UPGRADE_TIMELOCK_SECONDS: u64 = 86_400;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EscrowId(pub Address, pub u64);

#[contracttype]
#[derive(Clone, Debug)]
pub struct EscrowCreatedEvent {
    pub escrow_id: u64,
    pub payer: Address,
    pub payee: Address,
    pub amount: i128,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct EscrowReleasedEvent {
    pub escrow_id: u64,
    pub payee: Address,
    pub amount: i128,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct EscrowRefundedEvent {
    pub escrow_id: u64,
    pub payer: Address,
    pub amount: i128,
    pub timestamp: u64,
}

#[contract]
pub struct Escrow;

#[contractimpl]
impl Escrow {
    fn get_admin(env: &Env) -> Result<Address, EscrowError> {
        env.storage()
            .instance()
            .get(&DATA_KEY.admin)
            .ok_or(EscrowError::UninitializedAdmin)
    }

    fn get_acbu_token(env: &Env) -> Result<Address, EscrowError> {
        env.storage()
            .instance()
            .get(&DATA_KEY.acbu_token)
            .ok_or(EscrowError::UninitializedAcBuToken)
    }

    /// Initialize the escrow contract
    pub fn initialize(env: Env, admin: Address, acbu_token: Address) {
        if env.storage().instance().has(&DATA_KEY.admin) {
            env.panic_with_error(EscrowError::AlreadyInitialized);
        }
        env.storage().instance().set(&DATA_KEY.admin, &admin);
        env.storage()
            .instance()
            .set(&DATA_KEY.acbu_token, &acbu_token);
        env.storage().instance().set(&DATA_KEY.paused, &false);
        env.storage()
            .instance()
            .set(&SharedDataKey::Version, &CONTRACT_VERSION);
    }

    /// Create escrow: payer deposits ACBU, payee can claim after release
    /// Escrow ID is unique per payer and provided by caller to prevent collisions
    pub fn create(
        env: Env,
        payer: Address,
        payee: Address,
        amount: i128,
        escrow_id: u64,
    ) -> Result<(), EscrowError> {
        // Re-entrancy guard
        reentrancy_guard::acquire_guard(&env);

        let paused: bool = env
            .storage()
            .instance()
            .get(&DATA_KEY.paused)
            .unwrap_or(false);
        if paused {
            return Err(EscrowError::Paused);
        }
        if amount <= 0 {
            return Err(EscrowError::InvalidAmount);
        }
        payer.require_auth();
        let key = EscrowId(payer.clone(), escrow_id);

        if env.storage().temporary().has(&key) {
            return Err(EscrowError::EscrowExists);
        }

        let acbu = Self::get_acbu_token(&env)?;
        let client = soroban_sdk::token::Client::new(&env, &acbu);

        // CEI: write state before the external token transfer so any token-level
        // callback observes the escrow as already recorded.
        env.storage()
            .temporary()
            .set(&key, &(payer.clone(), payee.clone(), amount));

        client.transfer(&payer, &env.current_contract_address(), &amount);

        env.events().publish(
            (symbol_short!("esc_crtd"), escrow_id),
            EscrowCreatedEvent {
                escrow_id,
                payer,
                payee,
                amount,
                timestamp: env.ledger().timestamp(),
            },
        );

        // Release re-entrancy guard
        reentrancy_guard::release_guard(&env);

        Ok(())
    }

    /// Release escrow: payee receives ACBU (payer authorization required)
    /// caller must supply payer and escrow_id to identify which escrow to release
    pub fn release(env: Env, escrow_id: u64, payer: Address) -> Result<(), EscrowError> {
        // Re-entrancy guard
        reentrancy_guard::acquire_guard(&env);

        let paused: bool = env
            .storage()
            .instance()
            .get(&DATA_KEY.paused)
            .unwrap_or(false);
        if paused {
            return Err(EscrowError::Paused);
        }

        payer.require_auth();

        let key = EscrowId(payer.clone(), escrow_id);
        let (stored_payer, payee, amount): (Address, Address, i128) = env
            .storage()
            .temporary()
            .get(&key)
            .ok_or(EscrowError::EscrowNotFound)?;
        if stored_payer != payer {
            return Err(EscrowError::PayerMismatch);
        }

        let acbu = Self::get_acbu_token(&env)?;
        let client = soroban_sdk::token::Client::new(&env, &acbu);

        // CEI: remove the escrow record before the external transfer so the
        // escrow cannot be claimed a second time if the token executes a callback.
        env.storage().temporary().remove(&key);

        client.transfer(&env.current_contract_address(), &payee, &amount);

        env.events().publish(
            (symbol_short!("esc_rel"), escrow_id),
            EscrowReleasedEvent {
                escrow_id,
                payee,
                amount,
                timestamp: env.ledger().timestamp(),
            },
        );

        // Release re-entrancy guard
        reentrancy_guard::release_guard(&env);

        Ok(())
    }

    /// Refund escrow: payer gets ACBU back (admin or dispute resolution)
    /// key is same as release since it identifies which escrow to refund
    pub fn refund(env: Env, escrow_id: u64, payer: Address) -> Result<(), EscrowError> {
        // Re-entrancy guard
        reentrancy_guard::acquire_guard(&env);

        let admin = Self::get_admin(&env)?;
        admin.require_auth();

        let key = EscrowId(payer.clone(), escrow_id);
        let (stored_payer, _payee, amount): (Address, Address, i128) = env
            .storage()
            .temporary()
            .get(&key)
            .ok_or(EscrowError::EscrowNotFound)?;

        if stored_payer != payer {
            return Err(EscrowError::PayerMismatch);
        }

        let acbu = Self::get_acbu_token(&env)?;
        let client = soroban_sdk::token::Client::new(&env, &acbu);

        // CEI: remove the escrow record before the external transfer so the
        // escrow cannot be refunded twice if the token executes a callback.
        env.storage().temporary().remove(&key);

        client.transfer(&env.current_contract_address(), &payer, &amount);

        env.events().publish(
            (symbol_short!("esc_ref"), escrow_id),
            EscrowRefundedEvent {
                escrow_id,
                payer,
                amount,
                timestamp: env.ledger().timestamp(),
            },
        );

        // Release re-entrancy guard
        reentrancy_guard::release_guard(&env);

        Ok(())
    }

    pub fn pause(env: Env) -> Result<(), EscrowError> {
        let admin = Self::get_admin(&env)?;
        admin.require_auth();
        env.storage().instance().set(&DATA_KEY.paused, &true);
        Ok(())
    }

    pub fn unpause(env: Env) -> Result<(), EscrowError> {
        let admin = Self::get_admin(&env)?;
        admin.require_auth();
        env.storage().instance().set(&DATA_KEY.paused, &false);
        Ok(())
    }

    pub fn update_acbu_token(env: Env, new_acbu_token: Address) -> Result<(), EscrowError> {
        let admin = Self::get_admin(&env)?;
        admin.require_auth();
        env.storage()
            .instance()
            .set(&DATA_KEY.acbu_token, &new_acbu_token);
        Ok(())
    }

    pub fn version(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&SharedDataKey::Version)
            .unwrap_or(0)
    }

    pub fn migrate(env: Env) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.admin)
            .unwrap_or_else(|| env.panic_with_error(EscrowError::UninitializedAdmin));
        admin.require_auth();

        let current_version = CONTRACT_VERSION;
        let stored_version: u32 = env
            .storage()
            .instance()
            .get(&SharedDataKey::Version)
            .unwrap_or(0);
        if stored_version < current_version {
            env.storage()
                .instance()
                .set(&SharedDataKey::Version, &current_version);
        }
    }

    pub fn propose_upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), EscrowError> {
        let admin = Self::get_admin(&env)?;
        admin.require_auth();
        let eligible_at = env.ledger().timestamp() + UPGRADE_TIMELOCK_SECONDS;
        env.storage()
            .instance()
            .set(&DATA_KEY.pending_upgrade, &new_wasm_hash);
        env.storage()
            .instance()
            .set(&DATA_KEY.pending_upgrade_eligible_at, &eligible_at);
        Ok(())
    }

    pub fn execute_upgrade(env: Env) -> Result<(), EscrowError> {
        let admin = Self::get_admin(&env)?;
        admin.require_auth();
        let wasm_hash: BytesN<32> = env
            .storage()
            .instance()
            .get(&DATA_KEY.pending_upgrade)
            .ok_or(EscrowError::NoPendingUpgrade)?;
        let eligible_at: u64 = env
            .storage()
            .instance()
            .get(&DATA_KEY.pending_upgrade_eligible_at)
            .unwrap_or(u64::MAX);
        if env.ledger().timestamp() < eligible_at {
            return Err(EscrowError::TimelockNotElapsed);
        }
        env.storage().instance().remove(&DATA_KEY.pending_upgrade);
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_upgrade_eligible_at);
        env.deployer().update_current_contract_wasm(wasm_hash);
        Ok(())
    }

    pub fn cancel_upgrade(env: Env) -> Result<(), EscrowError> {
        let admin = Self::get_admin(&env)?;
        admin.require_auth();
        env.storage().instance().remove(&DATA_KEY.pending_upgrade);
        env.storage()
            .instance()
            .remove(&DATA_KEY.pending_upgrade_eligible_at);
        Ok(())
    }
}
