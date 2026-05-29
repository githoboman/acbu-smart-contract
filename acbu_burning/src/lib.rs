use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, vec, Address, Env,
    IntoVal, Symbol, Vec,
};

use shared::{
    calculate_fee, ContractError, CurrencyCode, DataKey as SharedDataKey, BASIS_POINTS,
    DECIMALS, MIN_BURN_AMOUNT, ORACLE_GET_CURRENCIES, ORACLE_GET_BASKET_WEIGHT,
    ORACLE_GET_RATE, ORACLE_GET_S_TOKEN_ADDR,
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
    pub fn initialize(env: Env, admin: Address, oracle: Address, reserve_tracker: Address, acbu_token: Address, withdrawal_processor: Address, vault: Address, fee_rate_bps: i128, fee_single_redeem_bps: i128) {
        if env.storage().instance().has(&DATA_KEY.admin) { env.panic_with_error(ContractError::Unauthorized); }
        env.storage().instance().set(&DATA_KEY.admin, &admin);
        env.storage().instance().set(&DATA_KEY.oracle, &oracle);
        env.storage().instance().set(&DATA_KEY.reserve_tracker, &reserve_tracker);
        env.storage().instance().set(&DATA_KEY.acbu_token, &acbu_token);
        env.storage().instance().set(&DATA_KEY.withdrawal_processor, &withdrawal_processor);
        env.storage().instance().set(&DATA_KEY.vault, &vault);
        env.storage().instance().set(&DATA_KEY.fee_rate, &fee_rate_bps);
        env.storage().instance().set(&DATA_KEY.fee_single_redeem, &fee_single_redeem_bps);
        env.storage().instance().set(&SharedDataKey::Version, &2u32);
        env.storage().instance().set(&DATA_KEY.paused, &false);
        env.storage().instance().set(&DATA_KEY.min_burn_amount, &MIN_BURN_AMOUNT);
    }

    pub fn redeem_basket(env: Env, user: Address, recipients: Vec<Address>, acbu_amount: i128) -> Vec<i128> {
        Self::check_paused(&env);
        user.require_auth();

        let oracle_addr: Address = env.storage().instance().get(&DATA_KEY.oracle).unwrap();
        let vault: Address = env.storage().instance().get(&DATA_KEY.vault).unwrap();
        let _acbu_token: Address = env.storage().instance().get(&DATA_KEY.acbu_token).unwrap();
        let fee_rate: i128 = env.storage().instance().get(&DATA_KEY.fee_rate).unwrap();
        let _reserve_tracker_addr: Address = env.storage().instance().get(&DATA_KEY.reserve_tracker).unwrap();

        let total_fee = calculate_fee(acbu_amount, fee_rate);
        let net_acbu = acbu_amount.checked_sub(total_fee).expect("Underflow");
        let usd_total = net_acbu.checked_mul(100).and_then(|v| v.checked_div(DECIMALS)).expect("Overflow");

        let currencies: Vec<CurrencyCode> = env.invoke_contract(&oracle_addr, &Symbol::new(&env, ORACLE_GET_CURRENCIES), vec![&env]);
        
        let mut amounts_out = Vec::new(&env);
        for i in 0..currencies.len() {
            let currency = currencies.get(i).unwrap();
            let recipient = recipients.get(i).unwrap();

            let weight: i128 = env.invoke_contract(&oracle_addr, &Symbol::new(&env, ORACLE_GET_BASKET_WEIGHT), vec![&env, currency.clone().into_val(&env)]);
            let rate: i128 = env.invoke_contract(&oracle_addr, &Symbol::new(&env, ORACLE_GET_RATE), vec![&env, currency.clone().into_val(&env)]);
            let stoken: Address = env.invoke_contract(&oracle_addr, &Symbol::new(&env, ORACLE_GET_S_TOKEN_ADDR), vec![&env, currency.clone().into_val(&env)]);

            let acbu_gross_i = (weight * acbu_amount) / BASIS_POINTS;
            let fee_i = (weight * total_fee) / BASIS_POINTS;
            let _net_acbu_i = acbu_gross_i - fee_i;
            let usd_i = (weight * usd_total) / BASIS_POINTS;
            let native_i = (usd_i * DECIMALS) / rate;

            if native_i > 0 {
                let token = soroban_sdk::token::Client::new(&env, &stoken);
                token.transfer_from(&env.current_contract_address(), &vault, &recipient, &native_i);
            }
            amounts_out.push_back(native_i);
        }
        amounts_out
    }

    fn check_paused(env: &Env) {
        if env.storage().instance().get::<_, bool>(&DATA_KEY.paused).unwrap_or(false) {
            env.panic_with_error(ContractError::Paused);
        }
    }

    fn check_admin(env: &Env) {
        let admin: Address = env.storage().instance().get(&DATA_KEY.admin).unwrap();
        admin.require_auth();
    }
}