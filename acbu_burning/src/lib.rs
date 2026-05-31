use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, vec, Address, BytesN, Env, IntoVal,
    String as SorobanString, Symbol, Vec,
};

use shared::{
    calculate_fee, reentrancy_guard, BurnEvent, ContractError, CurrencyCode,
    DataKey as SharedDataKey, CONTRACT_VERSION, MIN_BURN_AMOUNT, ORACLE_GET_ACBU_RATE_WITH_TS,
    ORACLE_GET_BASKET_WEIGHT, ORACLE_GET_CURRENCIES, ORACLE_GET_RATE_WITH_TS,
    ORACLE_GET_S_TOKEN_ADDR, UPDATE_INTERVAL_SECONDS,
};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataKey {
    pub admin: Symbol,
    pub oracle: Symbol,
    pub reserve_tracker: Symbol,
    pub acbu_token: Symbol,
    pub withdrawal_processor: Symbol,
    pub vault: Symbol,
    pub fee_rate: Symbol,
    pub fee_single_redeem: Symbol,
    pub paused: Symbol,
    pub min_burn_amount: Symbol,
}

const DATA_KEY: DataKey = DataKey {
    admin: symbol_short!("ADMIN"),
    oracle: symbol_short!("ORACLE"),
    reserve_tracker: symbol_short!("RES_TRK"),
    acbu_token: symbol_short!("ACBU_TKN"),
    withdrawal_processor: symbol_short!("WD_PROC"),
    vault: symbol_short!("VAULT"),
    fee_rate: symbol_short!("FEE_RATE"),
    fee_single_redeem: symbol_short!("FEE_S_R"),
    paused: symbol_short!("PAUSED"),
    min_burn_amount: symbol_short!("MIN_BURN"),
};

#[contract]
pub struct BurningContract;

#[contractimpl]
impl BurningContract {
    pub fn initialize(
        env: Env,
        admin: Address,
        oracle: Address,
        reserve_tracker: Address,
        acbu_token: Address,
        withdrawal_processor: Address,
        vault: Address,
        fee_rate_bps: i128,
        fee_single_redeem_bps: i128,
    ) {
        if env.storage().instance().has(&DATA_KEY.admin) {
            env.panic_with_error(ContractError::Unauthorized);
        }

        env.storage().instance().set(&DATA_KEY.admin, &admin);
        env.storage().instance().set(&DATA_KEY.oracle, &oracle);
        env.storage()
            .instance()
            .set(&DATA_KEY.reserve_tracker, &reserve_tracker);
        env.storage()
            .instance()
            .set(&DATA_KEY.acbu_token, &acbu_token);
        env.storage()
            .instance()
            .set(&DATA_KEY.withdrawal_processor, &withdrawal_processor);
        env.storage().instance().set(&DATA_KEY.vault, &vault);
        env.storage()
            .instance()
            .set(&DATA_KEY.fee_rate, &fee_rate_bps);
        env.storage()
            .instance()
            .set(&DATA_KEY.fee_single_redeem, &fee_single_redeem_bps);
        env.storage()
            .instance()
            .set(&SharedDataKey::Version, &CONTRACT_VERSION);
        env.storage().instance().set(&DATA_KEY.paused, &false);
        env.storage()
            .instance()
            .set(&DATA_KEY.min_burn_amount, &MIN_BURN_AMOUNT);
    }

    /// Redeem ACBU for a single Afreum S-token (higher fee tier). Requires vault approval.
    pub fn redeem_single(
        env: Env,
        user: Address,
        recipient: Address,
        acbu_amount: i128,
        currency: CurrencyCode,
    ) -> i128 {
        reentrancy_guard::acquire_guard(&env);

        Self::check_paused(&env);
        user.require_auth();

        let min_amount: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.min_burn_amount)
            .unwrap();
        if acbu_amount < min_amount {
            env.panic_with_error(ContractError::InvalidAmount);
        }

        let oracle_addr: Address = env.storage().instance().get(&DATA_KEY.oracle).unwrap();
        let vault: Address = env.storage().instance().get(&DATA_KEY.vault).unwrap();
        let acbu_token: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let fee_single: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.fee_single_redeem)
            .unwrap();
        let reserve_tracker_addr: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.reserve_tracker)
            .unwrap();

        let current_time = env.ledger().timestamp();
        let (acbu_rate, oracle_timestamp): (i128, u64) = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, ORACLE_GET_ACBU_RATE_WITH_TS),
            vec![&env],
        );
        if current_time > oracle_timestamp.saturating_add(UPDATE_INTERVAL_SECONDS) {
            env.panic_with_error(ContractError::OracleError);
        }

        let (rate, rate_timestamp): (i128, u64) = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, ORACLE_GET_RATE_WITH_TS),
            vec![&env, currency.clone().into_val(&env)],
        );
        if current_time > rate_timestamp.saturating_add(UPDATE_INTERVAL_SECONDS) {
            env.panic_with_error(ContractError::OracleError);
        }

        if rate <= 0 || acbu_rate <= 0 {
            env.panic_with_error(ContractError::InvalidRate);
        }

        let stoken: Address = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, ORACLE_GET_S_TOKEN_ADDR),
            vec![&env, currency.clone().into_val(&env)],
        );

        let fee = calculate_fee(acbu_amount, fee_single);
        let net_acbu = acbu_amount
            .checked_sub(fee)
            .expect("Underflow in net acbu calculation");
        let stoken_out = net_acbu
            .checked_mul(acbu_rate)
            .and_then(|v| v.checked_div(rate))
            .expect("Overflow in stoken out calculation");

        Self::check_reserves(&env, &acbu_token, &reserve_tracker_addr);

        let acbu_client = soroban_sdk::token::Client::new(&env, &acbu_token);
        acbu_client.burn(&user, &acbu_amount);

        let token = soroban_sdk::token::Client::new(&env, &stoken);
        let spender = env.current_contract_address();
        token.transfer_from(&spender, &vault, &recipient, &stoken_out);

        let burn_event = BurnEvent {
            transaction_id: SorobanString::from_str(&env, "redeem_single"),
            user: user.clone(),
            acbu_amount,
            net_acbu,
            local_amount: stoken_out,
            currency,
            fee,
            rate,
            timestamp: env.ledger().timestamp(),
        };
        env.events()
            .publish((symbol_short!("burn"), user), burn_event);

        reentrancy_guard::release_guard(&env);
        stoken_out
    }

    /// Redeem ACBU for proportional Afreum S-tokens across the basket (lower fee tier).
    pub fn redeem_basket(
        env: Env,
        user: Address,
        recipients: Vec<Address>,
        acbu_amount: i128,
    ) -> Vec<i128> {
        reentrancy_guard::acquire_guard(&env);

        Self::check_paused(&env);
        user.require_auth();

        if recipients.is_empty() {
            env.panic_with_error(ContractError::InvalidRecipient);
        }

        let rlen = recipients.len();
        for i in 0..rlen {
            for j in (i + 1)..rlen {
                if recipients.get(i).unwrap() == recipients.get(j).unwrap() {
                    env.panic_with_error(ContractError::InvalidRecipient);
                }
            }
        }

        let min_amount: i128 = env
            .storage()
            .instance()
            .get(&DATA_KEY.min_burn_amount)
            .unwrap();
        if acbu_amount < min_amount {
            env.panic_with_error(ContractError::InvalidAmount);
        }

        let oracle_addr: Address = env.storage().instance().get(&DATA_KEY.oracle).unwrap();
        let vault: Address = env.storage().instance().get(&DATA_KEY.vault).unwrap();
        let acbu_token: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let fee_rate: i128 = env.storage().instance().get(&DATA_KEY.fee_rate).unwrap();
        let reserve_tracker_addr: Address = env
            .storage()
            .instance()
            .get(&DATA_KEY.reserve_tracker)
            .unwrap();

        let current_time = env.ledger().timestamp();
        let (acbu_rate, oracle_timestamp): (i128, u64) = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, ORACLE_GET_ACBU_RATE_WITH_TS),
            vec![&env],
        );
        if current_time > oracle_timestamp.saturating_add(UPDATE_INTERVAL_SECONDS) {
            env.panic_with_error(ContractError::OracleError);
        }
        if acbu_rate <= 0 {
            env.panic_with_error(ContractError::InvalidRate);
        }

        let currencies: Vec<CurrencyCode> = env.invoke_contract(
            &oracle_addr,
            &Symbol::new(&env, ORACLE_GET_CURRENCIES),
            vec![&env],
        );
        if currencies.is_empty() {
            env.panic_with_error(ContractError::InvalidCurrency);
        }
        if recipients.len() != currencies.len() {
            env.panic_with_error(ContractError::InvalidRecipient);
        }

        let mut weights = Vec::new(&env);
        let mut total_weight = 0i128;
        let mut last_positive_weight_index = None;
        for i in 0..currencies.len() {
            let currency = currencies.get(i).unwrap();
            let weight: i128 = env.invoke_contract(
                &oracle_addr,
                &Symbol::new(&env, ORACLE_GET_BASKET_WEIGHT),
                vec![&env, currency.into_val(&env)],
            );
            if weight < 0 {
                env.panic_with_error(ContractError::InvalidAmount);
            }
            if weight > 0 {
                total_weight = total_weight
                    .checked_add(weight)
                    .expect("Overflow in total basket weight");
                last_positive_weight_index = Some(i);
            }
            weights.push_back(weight);
        }
        if total_weight <= 0 {
            env.panic_with_error(ContractError::InvalidCurrency);
        }

        let total_fee = calculate_fee(acbu_amount, fee_rate);
        let net_acbu = acbu_amount
            .checked_sub(total_fee)
            .expect("Underflow in net acbu calculation");

        Self::check_reserves(&env, &acbu_token, &reserve_tracker_addr);

        let acbu_client = soroban_sdk::token::Client::new(&env, &acbu_token);
        acbu_client.burn(&user, &acbu_amount);

        let mut amounts_out = Vec::new(&env);
        let mut allocated_gross = 0i128;
        let mut allocated_fee = 0i128;
        let last_positive_weight_index = last_positive_weight_index.unwrap();

        for i in 0..currencies.len() {
            let currency = currencies.get(i).unwrap();
            let recipient = recipients.get(i).unwrap();
            let weight = weights.get(i).unwrap();

            if weight == 0 {
                amounts_out.push_back(0);
                continue;
            }

            let (acbu_gross_i, fee_i) = if i == last_positive_weight_index {
                (
                    acbu_amount
                        .checked_sub(allocated_gross)
                        .expect("Underflow in remaining gross allocation"),
                    total_fee
                        .checked_sub(allocated_fee)
                        .expect("Underflow in remaining fee allocation"),
                )
            } else {
                (
                    Self::weighted_floor(acbu_amount, weight, total_weight),
                    Self::weighted_floor(total_fee, weight, total_weight),
                )
            };
            let net_acbu_i = acbu_gross_i
                .checked_sub(fee_i)
                .expect("Underflow in net basket allocation");
            allocated_gross = allocated_gross
                .checked_add(acbu_gross_i)
                .expect("Overflow in gross allocation");
            allocated_fee = allocated_fee
                .checked_add(fee_i)
                .expect("Overflow in fee allocation");

            let (rate, rate_timestamp): (i128, u64) = env.invoke_contract(
                &oracle_addr,
                &Symbol::new(&env, ORACLE_GET_RATE_WITH_TS),
                vec![&env, currency.clone().into_val(&env)],
            );
            if current_time > rate_timestamp.saturating_add(UPDATE_INTERVAL_SECONDS) {
                env.panic_with_error(ContractError::OracleError);
            }
            if rate <= 0 {
                env.panic_with_error(ContractError::InvalidRate);
            }

            let stoken: Address = env.invoke_contract(
                &oracle_addr,
                &Symbol::new(&env, ORACLE_GET_S_TOKEN_ADDR),
                vec![&env, currency.clone().into_val(&env)],
            );
            let native_i = net_acbu_i
                .checked_mul(acbu_rate)
                .and_then(|v| v.checked_div(rate))
                .expect("Overflow in native basket calculation");

            if native_i > 0 {
                let token = soroban_sdk::token::Client::new(&env, &stoken);
                let spender = env.current_contract_address();
                token.transfer_from(&spender, &vault, &recipient, &native_i);
            }
            amounts_out.push_back(native_i);

            let burn_event = BurnEvent {
                transaction_id: SorobanString::from_str(&env, "redeem_basket"),
                user: user.clone(),
                acbu_amount: acbu_gross_i,
                net_acbu: net_acbu_i,
                local_amount: native_i,
                currency,
                fee: fee_i,
                rate,
                timestamp: env.ledger().timestamp(),
            };
            env.events()
                .publish((symbol_short!("burn"), user.clone()), burn_event);
        }

        if allocated_gross != acbu_amount || allocated_fee != total_fee {
            env.panic_with_error(ContractError::InvalidAmount);
        }
        if allocated_gross
            .checked_sub(allocated_fee)
            .expect("Underflow in allocated net validation")
            != net_acbu
        {
            env.panic_with_error(ContractError::InvalidAmount);
        }

        reentrancy_guard::release_guard(&env);
        amounts_out
    }

    pub fn version(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&SharedDataKey::Version)
            .unwrap_or(0)
    }

    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>, new_version: u32) {
        Self::check_admin(&env);

        let current_version = Self::version(env.clone());
        if new_version <= current_version {
            env.panic_with_error(ContractError::InvalidVersion);
        }

        env.deployer().update_current_contract_wasm(new_wasm_hash);

        for v in current_version..new_version {
            if v == 0 {
                migrate_v0_to_v1(env.clone());
            }
        }

        env.storage()
            .instance()
            .set(&SharedDataKey::Version, &new_version);
    }

    fn weighted_floor(total: i128, weight: i128, total_weight: i128) -> i128 {
        total
            .checked_mul(weight)
            .and_then(|v| v.checked_div(total_weight))
            .expect("Overflow in weighted allocation")
    }

    fn check_reserves(env: &Env, acbu_token: &Address, reserve_tracker_addr: &Address) {
        let current_supply: i128 =
            env.invoke_contract(acbu_token, &Symbol::new(env, "get_total_supply"), vec![env]);
        let reserve_ok: bool = env.invoke_contract(
            reserve_tracker_addr,
            &Symbol::new(env, "is_reserve_sufficient"),
            vec![env, current_supply.into_val(env)],
        );
        if !reserve_ok {
            env.panic_with_error(ContractError::InsufficientReserves);
        }
    }

    fn check_paused(env: &Env) {
        if env
            .storage()
            .instance()
            .get::<_, bool>(&DATA_KEY.paused)
            .unwrap_or(false)
        {
            env.panic_with_error(ContractError::Paused);
        }
    }

    fn check_admin(env: &Env) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
    }
}

fn migrate_v0_to_v1(_env: Env) {
    // No storage schema changes between v0 and v1.
}
