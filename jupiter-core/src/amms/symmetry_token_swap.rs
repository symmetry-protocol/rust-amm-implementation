use anchor_lang::prelude::AccountMeta;
use anyhow::Result;
use std::collections::HashMap;

use crate::amms::accounts::{NUM_OF_POINTS_IN_CURVE_DATA, USE_CURVE_DATA, BPS_DIVIDER, LP_DISABLED};
use crate::amms::amm::{Amm, KeyedAccount};
use solana_sdk::{ pubkey, pubkey::Pubkey, instruction::Instruction};
use rust_decimal::Decimal;

use super::accounts::{FundState, CurveData, TokenList, OraclePrice, TokenPriceData, MAX_TOKENS_IN_ASSET_POOL, TokenSettings, WEIGHT_MULTIPLIER};
use super::amm::{Quote, QuoteParams, SwapLegAndAccountMetas, SwapParams};
use jupiter::jupiter_override::{Swap, SwapLeg};

pub struct SymmetryTokenSwap {
    key: Pubkey,
    label: String,
    fund_state: FundState,
    token_list: TokenList,
    curve_data: CurveData,
}

impl SymmetryTokenSwap {

    const SYMMETRY_PROGRAM_ADDRESS: Pubkey = pubkey!("2KehYt3KsEQR53jYcxjbQp2d2kCp4AkuQW68atufRwSr");
    const TOKEN_LIST_ADDRESS: Pubkey = pubkey!("3SnUughtueoVrhevXTLMf586qvKNNXggNsc7NgoMUU1t");
    const CURVE_DATA_ADDRESS: Pubkey = pubkey!("4QMjSHuM3iS7Fdfi8kZJfHRKoEJSDHEtEwqbChsTcUVK");
    const PDA_ADDRESS: Pubkey = pubkey!("BLBYiq48WcLQ5SxiftyKmPtmsZPUBEnDEjqEnKGAR4zx");
    const SWAP_FEE_ADDRESS: Pubkey = pubkey!("AWfpfzA6FYbqx4JLz75PDgsjH7jtBnnmJ6MXW5zNY2Ei");

    const ASSOCIATED_TOKEN_PROGRAM_ADDRESS: Pubkey = pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
    const SPL_TOKEN_PROGRAM_ADDRESS: Pubkey = pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

    const SYMMETRY_PROGRAM_SWAP_INSTRUCTION_ID: u64 = 219478785678209410;


    pub fn from_keyed_account(fund_state_account: &KeyedAccount, token_list_account: &KeyedAccount) -> Result<Self> {
        let fund_state = FundState::load(&fund_state_account.account.data);
        let token_list = TokenList::load(&token_list_account.account.data);

        let label = String::from("Symmetry");
        Ok(Self {
            key: fund_state_account.key,
            label: label,
            fund_state: fund_state,
            token_list: token_list,
            curve_data: CurveData::empty(),
        })
    }

    fn clone(&self) -> SymmetryTokenSwap {
        SymmetryTokenSwap {
            key: self.key,
            label: self.label.clone(),
            fund_state: FundState {
                manager: self.fund_state.manager,
                host_pubkey: self.fund_state.host_pubkey,
                num_of_tokens: self.fund_state.num_of_tokens,
                current_comp_token: self.fund_state.current_comp_token,
                current_comp_amount: self.fund_state.current_comp_amount,
                target_weight: self.fund_state.target_weight,
                weight_sum: self.fund_state.weight_sum,
                rebalance_threshold: self.fund_state.rebalance_threshold,
                lp_offset_threshold: self.fund_state.lp_offset_threshold
            },
            token_list: TokenList {
                num_tokens: self.token_list.num_tokens,
                list: self.token_list.list
            },
            curve_data: CurveData {
                buy: self.curve_data.buy,
                sell: self.curve_data.sell
            }
        }
    }

    pub fn mul_div(a: u64, b: u64, c: u64,) -> u64 {
        match c {
            0 => 0,
            _ => ((a as u128) * (b as u128) / (c as u128)) as u64
        }
    }

    pub fn amount_to_usd_value(amount: u64, decimals: u8, price: u64) -> u64 {
        SymmetryTokenSwap::mul_div(amount, price, u64::pow(10,decimals as u32))
    }

    pub fn usd_value_to_amount(worth: u64, decimals: u8, price: u64) -> u64 {
        SymmetryTokenSwap::mul_div(worth, u64::pow(10,decimals as u32), price)
    }

    pub fn compute_value_of_sold_token(
        amount: u64,
        token_settings: TokenSettings,
        price: OraclePrice,
        start_amount: u64,
        target_amount: u64,
        curve_data: TokenPriceData
    ) -> u64 {
        let mut current_amount = start_amount;
        let mut curve_offset = if start_amount > target_amount { start_amount - target_amount } else { 0 };
        let mut current_output_value: u64 = 0;
        let mut amount_left: u64 = amount;
        let mut current_price = price.sell_price;

        for step in 0..NUM_OF_POINTS_IN_CURVE_DATA+1 {
            let step_amount = if step < NUM_OF_POINTS_IN_CURVE_DATA
                { curve_data.amount[step] } else { amount_left };
            if step < NUM_OF_POINTS_IN_CURVE_DATA && curve_data.price[step] < current_price {
                if token_settings.use_curve_data == USE_CURVE_DATA
                    { current_price = curve_data.price[step]; }
            }
            if step == NUM_OF_POINTS_IN_CURVE_DATA { curve_offset = 0; }
            if step_amount <= curve_offset {
                curve_offset -= curve_data.amount[step];
                continue;
            }
            let mut amount_in_interval = step_amount - curve_offset;
            curve_offset = 0;
            if amount_in_interval > amount_left { amount_in_interval = amount_left };
            let mut amount_before_tw = amount_in_interval;
            if current_amount >= target_amount
                { amount_before_tw = 0; } else
            if current_amount + amount_in_interval >= target_amount
                { amount_before_tw -= current_amount + amount_in_interval - target_amount; }
            let amount_after_tw = amount_in_interval - amount_before_tw;
            let value_before_tw = SymmetryTokenSwap::amount_to_usd_value(
                amount_before_tw,
                token_settings.decimals,
                current_price
            );
            let value_after_tw = SymmetryTokenSwap::amount_to_usd_value(
                amount_after_tw,
                token_settings.decimals,
                current_price
            );
            let fees =
                SymmetryTokenSwap::mul_div(value_before_tw, token_settings.token_swap_fee_before_tw_bps as u64, BPS_DIVIDER) +
                SymmetryTokenSwap::mul_div(value_after_tw, token_settings.token_swap_fee_after_tw_bps as u64, BPS_DIVIDER);
            current_output_value += value_before_tw + value_after_tw - fees;
            amount_left -= amount_in_interval;
            current_amount += amount_in_interval;
            if amount_left == 0 { break; }
        };
        
        current_output_value
    }

    pub fn compute_amount_of_bought_token(
        value: u64,
        token_settings: TokenSettings,
        price: OraclePrice,
        start_amount: u64,
        target_amount: u64,
        curve_data: TokenPriceData,
    ) -> u64 {
        let mut current_amount = start_amount;
        let mut curve_offset = if start_amount < target_amount { target_amount - start_amount } else { 0 };
        let mut current_output_amount: u64 = 0;
        let mut value_left: u64 = value;
        let mut current_price = price.buy_price;

        for step in 0..NUM_OF_POINTS_IN_CURVE_DATA+1 {
            let step_amount = if step < NUM_OF_POINTS_IN_CURVE_DATA
                { curve_data.amount[step] } else
                { SymmetryTokenSwap::usd_value_to_amount(value_left * 2, token_settings.decimals, current_price) };
            if step < NUM_OF_POINTS_IN_CURVE_DATA && curve_data.price[step] > current_price {
                if token_settings.use_curve_data == USE_CURVE_DATA { current_price = curve_data.price[step]; };
            }
            if step == NUM_OF_POINTS_IN_CURVE_DATA { curve_offset = 0; }
            if step_amount <= curve_offset {
                curve_offset -= curve_data.amount[step];
                continue;
            }
            let mut amount_in_interval = step_amount - curve_offset;
            curve_offset = 0;

            let mut value_in_interval = SymmetryTokenSwap::amount_to_usd_value(amount_in_interval, token_settings.decimals, current_price);
            if value_in_interval > value_left {
                value_in_interval = value_left;
                amount_in_interval = SymmetryTokenSwap::usd_value_to_amount(value_in_interval, token_settings.decimals, current_price);
            }

            let mut value_before_tw = value_in_interval;
            if current_amount <= target_amount
                { value_before_tw = 0; } else
            if current_amount <= target_amount + amount_in_interval
                { value_before_tw -= SymmetryTokenSwap::amount_to_usd_value(target_amount + amount_in_interval - current_amount, token_settings.decimals, current_price)}
            let value_after_tw = value_in_interval - value_before_tw;

            let fees =
                SymmetryTokenSwap::mul_div(value_before_tw, token_settings.token_swap_fee_before_tw_bps as u64, BPS_DIVIDER) +
                SymmetryTokenSwap::mul_div(value_after_tw, token_settings.token_swap_fee_after_tw_bps as u64, BPS_DIVIDER);
            
            let amount_bought = SymmetryTokenSwap::usd_value_to_amount(value_in_interval - fees, token_settings.decimals, current_price);
            
            current_output_amount += amount_bought;
            value_left -= value_in_interval;
            if amount_bought > current_amount
                { current_amount = 0; } else { current_amount -= amount_bought; }
            if value_left == 0 { break; }
        };

        current_output_amount
    }

    
}

impl Amm for SymmetryTokenSwap {
    fn label(&self) -> String {
        String::from("Symmetry")
    }

    fn key(&self) -> Pubkey {
        self.key
    }

    fn get_reserve_mints(&self) -> Vec<Pubkey> {
        let mut vec: Vec<Pubkey> = Vec::new();
        for i in 0..self.fund_state.num_of_tokens as usize {
            if self.token_list.list[self.fund_state.current_comp_token[i] as usize].lp_on != LP_DISABLED {
                vec.push(self.token_list.list[self.fund_state.current_comp_token[i] as usize].token_mint)
            }
        }
        return vec;
    }

    fn get_accounts_to_update(&self) -> Vec<Pubkey> {
        let mut accounts_to_update: Vec<Pubkey> = Vec::new();
        accounts_to_update.push(SymmetryTokenSwap::CURVE_DATA_ADDRESS);
        accounts_to_update.push(self.key);
        for i in 0..MAX_TOKENS_IN_ASSET_POOL {
            if self.token_list.list[i].oracle_account != Pubkey::default() {
                accounts_to_update.push(self.token_list.list[i].oracle_account)
            }
        }
        return accounts_to_update;
    }

    fn update(&mut self, accounts_map: &HashMap<Pubkey, Vec<u8>>) -> Result<()> {
        self.curve_data = CurveData::load(accounts_map.get(&SymmetryTokenSwap::CURVE_DATA_ADDRESS).unwrap());
        self.fund_state = FundState::load(accounts_map.get(&self.key).unwrap());
        for i in 0..MAX_TOKENS_IN_ASSET_POOL {
            if self.token_list.list[i].oracle_account != Pubkey::default() {
                self.token_list.list[i].oracle_price = OraclePrice::load(
                    accounts_map.get(&self.token_list.list[i].oracle_account).unwrap(),
                    self.token_list.list[i]
                );
            }
        }

        Ok(())
    }

    fn quote(&self, quote_params: &QuoteParams) -> Result<Quote> {
        
        let fund_state = self.fund_state;
        let token_list = self.token_list;
        let curve_data = self.curve_data;

        let from_amount: u64 = quote_params.in_amount;
        let from_token_id: u64 = token_list.list.iter()
            .position(|&x| x.token_mint == quote_params.input_mint).unwrap() as u64;
        let to_token_id: u64 = token_list.list.iter()
            .position(|&x| x.token_mint == quote_params.output_mint).unwrap() as u64;
    
        let from_token_settings = token_list.list[from_token_id as usize];
        let to_token_settings = token_list.list[to_token_id as usize];
    
        // checking if both tokens are present in fund_state
        let from_token_index: usize = fund_state.current_comp_token.iter()
                            .position(|&x| x == (from_token_id as u64)).unwrap() as usize;
        let to_token_index: usize = fund_state.current_comp_token.iter()
                            .position(|&x| x == (to_token_id as u64)).unwrap() as usize;

        let mut fund_worth = 0;
        // calculating the fund_worth
        for i in 0..(fund_state.num_of_tokens as usize) {
            let token = fund_state.current_comp_token[i] as usize;
            let token_settings = token_list.list[token];
            let token_price = token_settings.oracle_price;
            if token_price.oracle_live == 0 {
                panic!()
            }
            fund_worth += SymmetryTokenSwap::amount_to_usd_value(
                fund_state.current_comp_amount[i],
                token_settings.decimals,
                token_price.avg_price
            );
        }
    
        let from_token_price = from_token_settings.oracle_price;
        let to_token_price = to_token_settings.oracle_price;
        
        let from_token_target_amount: u64 = SymmetryTokenSwap::usd_value_to_amount(
            SymmetryTokenSwap::mul_div(fund_state.target_weight[from_token_index], fund_worth, fund_state.weight_sum),
            from_token_settings.decimals,
            from_token_price.avg_price
        );
        let to_token_target_amount: u64 = SymmetryTokenSwap::usd_value_to_amount(
            SymmetryTokenSwap::mul_div(fund_state.target_weight[to_token_index], fund_worth, fund_state.weight_sum),
            to_token_settings.decimals,
            to_token_price.avg_price,
        );
    
        let value = SymmetryTokenSwap::compute_value_of_sold_token(
            from_amount,
            from_token_settings,
            from_token_price,
            fund_state.current_comp_amount[from_token_index],
            from_token_target_amount,
            curve_data.sell[from_token_id as usize],
        );
    
        let mut to_amount = SymmetryTokenSwap::compute_amount_of_bought_token(
            value,
            to_token_settings,
            to_token_price,
            fund_state.current_comp_amount[to_token_index],
            to_token_target_amount,
            curve_data.buy[to_token_id as usize],
        );
    
        let mut amount_without_fees = SymmetryTokenSwap::usd_value_to_amount(
            SymmetryTokenSwap::amount_to_usd_value(
                from_amount,
                from_token_settings.decimals,
                from_token_price.sell_price
            ),
            to_token_settings.decimals,
            to_token_price.buy_price
        );
    
        let fair_amount = SymmetryTokenSwap::usd_value_to_amount(
            SymmetryTokenSwap::amount_to_usd_value(
                from_amount,
                from_token_settings.decimals,
                from_token_price.avg_price
            ),
            to_token_settings.decimals,
            to_token_price.avg_price
        );
    
        if amount_without_fees > fund_state.current_comp_amount[to_token_index] {
            amount_without_fees = fund_state.current_comp_amount[to_token_index];
        }
    
        if to_amount > amount_without_fees {
            to_amount = amount_without_fees
        }
    
        let total_fees = amount_without_fees - to_amount;
    
        let symmetry_bps = token_list.list[0].additional_data[60];
        let symmetry_fee = SymmetryTokenSwap::mul_div(total_fees, symmetry_bps as u64, 100);
    
        let host_bps = token_list.list[0].additional_data[61];
        let host_fee = SymmetryTokenSwap::mul_div(total_fees, host_bps as u64, 100);
    
        let manager_bps = token_list.list[0].additional_data[62];
        let manager_fee = SymmetryTokenSwap::mul_div(total_fees, manager_bps as u64, 100);
    
        let fund_fee = total_fees - symmetry_fee - host_fee - manager_fee;
    
        let confidence_bps = SymmetryTokenSwap::mul_div(
            fair_amount - amount_without_fees,
            BPS_DIVIDER * 100,
            fair_amount
        );
        let fee_bps = SymmetryTokenSwap::mul_div(
            amount_without_fees - to_amount,
            BPS_DIVIDER * 100,
            fair_amount
        );
        
        let from_token_worth_before_swap = SymmetryTokenSwap::amount_to_usd_value(
            fund_state.current_comp_amount[from_token_index],
            from_token_settings.decimals,
            from_token_price.avg_price
        );
        let to_token_worth_before_swap = SymmetryTokenSwap::amount_to_usd_value(
            fund_state.current_comp_amount[to_token_index],
            to_token_settings.decimals,
            to_token_price.avg_price
        );
    
        let from_token_worth_after_swap = SymmetryTokenSwap::amount_to_usd_value(
            fund_state.current_comp_amount[from_token_index] + from_amount,
            from_token_settings.decimals,
            from_token_price.avg_price
        );
        let to_token_worth_after_swap= SymmetryTokenSwap::amount_to_usd_value(
            fund_state.current_comp_amount[to_token_index] - (amount_without_fees - fund_fee),
            to_token_settings.decimals,
            to_token_price.avg_price
        );
    
        let from_old_weight = SymmetryTokenSwap::mul_div(
            from_token_worth_before_swap,
            WEIGHT_MULTIPLIER,
            fund_worth
        );
        let to_old_weight = SymmetryTokenSwap::mul_div(
            to_token_worth_before_swap,
            WEIGHT_MULTIPLIER,
            fund_worth
        );
    
        fund_worth = fund_worth - from_token_worth_before_swap;
        fund_worth = fund_worth - to_token_worth_before_swap;
        fund_worth = fund_worth + from_token_worth_after_swap;
        fund_worth = fund_worth + to_token_worth_after_swap;
    
        let from_new_weight = SymmetryTokenSwap::mul_div(
            from_token_worth_after_swap,
            WEIGHT_MULTIPLIER,
            fund_worth
        );
        let to_new_weight = SymmetryTokenSwap::mul_div(
            to_token_worth_after_swap,
            WEIGHT_MULTIPLIER,
            fund_worth
        );
    
        let allowed_offset = fund_state.rebalance_threshold * fund_state.lp_offset_threshold;
    
        let mut allowed_from_target_weight = SymmetryTokenSwap::mul_div(
            fund_state.target_weight[from_token_index],
            BPS_DIVIDER * BPS_DIVIDER + allowed_offset,
            BPS_DIVIDER * BPS_DIVIDER
        );
        let allowed_to_target_weight = SymmetryTokenSwap::mul_div(
            fund_state.target_weight[to_token_index],
            BPS_DIVIDER * BPS_DIVIDER - allowed_offset,
            BPS_DIVIDER * BPS_DIVIDER
        );
        if allowed_from_target_weight > WEIGHT_MULTIPLIER {
            allowed_from_target_weight = WEIGHT_MULTIPLIER;
        }
        
        let removing_dust =
            from_token_id == 0 as u64 &&
            fund_state.target_weight[to_token_index] == 0;

        if from_new_weight > allowed_from_target_weight && (!removing_dust) {
            panic!()
        }
        
        // checking if after swapping to_token's weight doesn't exceed target_weight
        if to_new_weight < allowed_to_target_weight {
            panic!()
        }

        Ok(Quote {
            in_amount: quote_params.in_amount,
            out_amount: to_amount,
            fee_amount: total_fees,
            fee_mint: quote_params.output_mint,
            fee_pct: Decimal::new(fee_bps as i64, 4),
            price_impact_pct: Decimal::new(confidence_bps as i64, 4),
            
            ..Quote::default()
        })
    }

    fn get_swap_leg_and_account_metas(
        &self,
        swap_params: &SwapParams,
    ) -> Result<SwapLegAndAccountMetas> {
        let SwapParams {
            destination_mint,
            in_amount,
            source_mint,
            user_destination_token_account,
            user_source_token_account,
            user_transfer_authority,
            open_order_address,
            quote_mint_to_referrer,
        } = swap_params;
        
        let from_token_id: u64 = self.token_list.list.iter().position(|&x| x.token_mint == *source_mint).unwrap() as u64;
        let to_token_id: u64 = self.token_list.list.iter().position(|&x| x.token_mint == *destination_mint).unwrap() as u64;

        let swap_to_fee: Pubkey = Pubkey::find_program_address(
            &[
                &SymmetryTokenSwap::SWAP_FEE_ADDRESS.to_bytes(),
                &SymmetryTokenSwap::SPL_TOKEN_PROGRAM_ADDRESS.to_bytes(),
                &destination_mint.to_bytes()
            ], 
            &SymmetryTokenSwap::ASSOCIATED_TOKEN_PROGRAM_ADDRESS
        ).0;
        let host_to_fee: Pubkey = Pubkey::find_program_address(
            &[
                &self.fund_state.host_pubkey.to_bytes(),
                &SymmetryTokenSwap::SPL_TOKEN_PROGRAM_ADDRESS.to_bytes(),
                &destination_mint.to_bytes()
            ], 
            &SymmetryTokenSwap::ASSOCIATED_TOKEN_PROGRAM_ADDRESS
        ).0;
        let manager_to_fee: Pubkey = Pubkey::find_program_address(
            &[
                &self.fund_state.manager.to_bytes(),
                &SymmetryTokenSwap::SPL_TOKEN_PROGRAM_ADDRESS.to_bytes(),
                &destination_mint.to_bytes()
            ], 
            &SymmetryTokenSwap::ASSOCIATED_TOKEN_PROGRAM_ADDRESS
        ).0;

        let mut account_metas: Vec<AccountMeta> = Vec::new();
        account_metas.push(AccountMeta::new(*user_transfer_authority, true));
        account_metas.push(AccountMeta::new(self.key, false));
        account_metas.push(AccountMeta::new_readonly(SymmetryTokenSwap::PDA_ADDRESS, false));
        account_metas.push(AccountMeta::new(self.token_list.list[from_token_id as usize].pda_token_account, false));
        account_metas.push(AccountMeta::new(*user_source_token_account, false));
        account_metas.push(AccountMeta::new(self.token_list.list[to_token_id as usize].pda_token_account, false));
        account_metas.push(AccountMeta::new(*user_destination_token_account, false));
        account_metas.push(AccountMeta::new(swap_to_fee, false));
        account_metas.push(AccountMeta::new(host_to_fee, false));
        account_metas.push(AccountMeta::new(manager_to_fee, false));
        account_metas.push(AccountMeta::new_readonly(SymmetryTokenSwap::TOKEN_LIST_ADDRESS, false));
        account_metas.push(AccountMeta::new_readonly(SymmetryTokenSwap::CURVE_DATA_ADDRESS, false));
        account_metas.push(AccountMeta::new_readonly(SymmetryTokenSwap::SPL_TOKEN_PROGRAM_ADDRESS, false));

        // Pyth Oracle accounts are being passed as remaining accounts
        for i in 0..self.fund_state.num_of_tokens as usize {
            account_metas.push(
                AccountMeta::new_readonly(self.token_list.list[self.fund_state.current_comp_token[i] as usize].oracle_account, false)
            );
        }

        let instruction_n: u64 = SymmetryTokenSwap::SYMMETRY_PROGRAM_SWAP_INSTRUCTION_ID;
        let minimum_amount_out: u64 = 0;
        let mut data = Vec::new();
        data.extend_from_slice(&instruction_n.to_le_bytes());
        data.extend_from_slice(&from_token_id.to_le_bytes());
        data.extend_from_slice(&to_token_id.to_le_bytes());
        data.extend_from_slice(&in_amount.to_le_bytes());
        data.extend_from_slice(&minimum_amount_out.to_le_bytes());
    
        let swap_instruction = Instruction {
            program_id: SymmetryTokenSwap::SYMMETRY_PROGRAM_ADDRESS,
            accounts: account_metas.clone(),
            data,
        };

        Ok(SwapLegAndAccountMetas {
            swap_leg: SwapLeg::Swap {
                swap: Swap::TokenSwap,
            },
            account_metas,
        })
    }

    fn clone_amm(&self) -> Box<dyn Amm + Send + Sync> {
        Box::new(self.clone())
    }
}

#[test]
fn test_symetry_token_swap() {
    const WSOL_TOKEN_MINT: Pubkey = pubkey!("So11111111111111111111111111111111111111112");
    const USDC_TOKEN_MINT: Pubkey = pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
    const USDT_TOKEN_MINT: Pubkey = pubkey!("Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB");
    const MSOL_TOKEN_MINT: Pubkey = pubkey!("mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So");

    use crate::amms::test_harness::AmmTestHarness;

    /* Init Token Swap */
    const TOKEN_LIST_ACCOUNT: Pubkey = SymmetryTokenSwap::TOKEN_LIST_ADDRESS;
    const FUND_STATE_ACCOUNT: Pubkey = pubkey!("2VHtUhF8KrjN4xx1fEsTB7Fcnw78DNHKwcjQF5ikFzqZ");

    let test_harness = AmmTestHarness::new();
    let fund_state_account = test_harness.get_keyed_account(FUND_STATE_ACCOUNT).unwrap();
    let token_list_account = test_harness.get_keyed_account(TOKEN_LIST_ACCOUNT).unwrap();
    let mut token_swap = SymmetryTokenSwap::from_keyed_account(&fund_state_account, &token_list_account).unwrap();

    /* Update TokenSwap (FundState + CurveData + Pyth Oracle accounts) */
    test_harness.update_amm(&mut token_swap);

    /* Token mints available for swap in a fund */
    println!("-------------------");
    let token_mints = token_swap.get_reserve_mints();
    println!("Available mints for swap: {:?}", token_mints);
    let from_token_mint: Pubkey = token_mints.clone().into_iter().find(|&x| x == MSOL_TOKEN_MINT).unwrap();
    let to_token_mint: Pubkey = token_mints.clone().into_iter().find(|&x| x == WSOL_TOKEN_MINT).unwrap(); 

    /* Get Quote */
    println!("-------------------");
    let in_amount: u64 = 1_000_000_000; // 1 MSOL -> ? WSOL
    let quote = token_swap
        .quote(&QuoteParams {
            input_mint: from_token_mint,
            in_amount: in_amount,
            output_mint: to_token_mint,
        })
        .unwrap();
    println!("Quote result: {:?}", quote);
    
    /* Get swap leg and account metas */
    println!("------------");
    let user = Pubkey::new_unique();
    let user_source = Pubkey::find_program_address(
        &[
            &user.to_bytes(),
            &SymmetryTokenSwap::SPL_TOKEN_PROGRAM_ADDRESS.to_bytes(),
            &from_token_mint.to_bytes()
        ], 
        &SymmetryTokenSwap::ASSOCIATED_TOKEN_PROGRAM_ADDRESS
    ).0;
    let user_destination = Pubkey::find_program_address(
        &[
            &user.to_bytes(),
            &SymmetryTokenSwap::SPL_TOKEN_PROGRAM_ADDRESS.to_bytes(),
            &to_token_mint.to_bytes()
        ], 
        &SymmetryTokenSwap::ASSOCIATED_TOKEN_PROGRAM_ADDRESS
    ).0;
    let _ = token_swap.get_swap_leg_and_account_metas(&SwapParams {
        source_mint: from_token_mint, 
        destination_mint: to_token_mint,
        user_source_token_account: user_source,
        user_destination_token_account: user_destination,
        user_transfer_authority: user,
        open_order_address: Option::None,
        quote_mint_to_referrer: Option::None,
        in_amount: in_amount
    });
}
