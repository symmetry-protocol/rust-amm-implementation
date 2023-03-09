use anchor_lang::prelude::AccountMeta;
use anyhow::Result;
use std::collections::HashMap;

use crate::amms::amm::{Amm, KeyedAccount};
use solana_sdk::{ pubkey, pubkey::Pubkey, instruction::Instruction};
use rust_decimal::Decimal;

use super::accounts::{FundState, CurveData, TokenInfo, SimplePrice, TokenPriceData, MAX_TOKENS_IN_ASSET_POOL};
use super::amm::{Quote, QuoteParams, SwapLegAndAccountMetas, SwapParams};
use jupiter::jupiter_override::{Swap, SwapLeg};

pub struct SymmetryTokenSwap {
    key: Pubkey,
    label: String,
    fund_state: FundState,
    token_info: TokenInfo,
    curve_data: CurveData,
}

impl SymmetryTokenSwap {

    const SYMMETRY_PROGRAM_ADDRESS: Pubkey = pubkey!("2KehYt3KsEQR53jYcxjbQp2d2kCp4AkuQW68atufRwSr");
    const TOKEN_INFO_ADDRESS: Pubkey = pubkey!("4Rn7pKKyiSNKZXKCoLqEpRznX1rhveV4dW1DCg6hRoVH");
    const CURVE_DATA_ADDRESS: Pubkey = pubkey!("4QMjSHuM3iS7Fdfi8kZJfHRKoEJSDHEtEwqbChsTcUVK");
    const PDA_ADDRESS: Pubkey = pubkey!("BLBYiq48WcLQ5SxiftyKmPtmsZPUBEnDEjqEnKGAR4zx");
    const SWAP_FEE_ADDRESS: Pubkey = pubkey!("AWfpfzA6FYbqx4JLz75PDgsjH7jtBnnmJ6MXW5zNY2Ei");

    const ASSOCIATED_TOKEN_PROGRAM_ADDRESS: Pubkey = pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
    const SPL_TOKEN_PROGRAM_ADDRESS: Pubkey = pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

    const SYMMETRY_PROGRAM_SWAP_INSTRUCTION_ID: u64 = 1448820615868184176;

    pub fn from_keyed_account(fund_state_account: &KeyedAccount, token_info_account: &KeyedAccount) -> Result<Self> {
        let fund_state = FundState::load(&fund_state_account.account.data);
        let token_info = TokenInfo::load(&token_info_account.account.data);

        let label = String::from("Symmetry");
        Ok(Self {
            key: fund_state_account.key,
            label: label,
            fund_state: fund_state,
            token_info: token_info,
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
            token_info: TokenInfo {
                token_mint: self.token_info.token_mint,
                pda_ta: self.token_info.pda_ta,
                oracle: self.token_info.oracle,
                decimals: self.token_info.decimals,
                oracle_price: self.token_info.oracle_price,
            },
            curve_data: CurveData {
                buy: self.curve_data.buy,
                sell: self.curve_data.sell
            }
        }
    }

    pub fn usd_value(amount: u64, decimals: u64, pyth_price: SimplePrice) -> u64 {
        let mut pow_den: u128 = u128::pow(10,decimals as u32 + (-pyth_price.expo) as u32);
        let mut pow_num: u128 = 1000000;
        if pow_den > pow_num {
            pow_den /= pow_num;
            pow_num = 1;
        } else {
            pow_num /= pow_den;
            pow_den = 1;
        }
        (amount as u128)
            .checked_mul(pyth_price.price as u128).unwrap()
            .checked_mul(pow_num as u128).unwrap()
            .checked_div(pow_den as u128).unwrap() as u64
    }

    pub fn amount_from_usd_value(usd_value: u64, decimals: u64, pyth_price: SimplePrice) -> u64 {
        let mut pow_den: u128 = u128::pow(10,decimals as u32 + (-pyth_price.expo) as u32);
        let mut pow_num: u128 = 1000000;
        if pow_den > pow_num {
            pow_den /= pow_num;
            pow_num = 1;
        } else {
            pow_num /= pow_den;
            pow_den = 1;
        }
        (usd_value as u128)
            .checked_mul(pow_den as u128).unwrap()
            .checked_div(pyth_price.price as u128).unwrap()
            .checked_div(pow_num as u128).unwrap() as u64
    }

    pub fn mul_div(a: u64, b: u64, c: u64) -> u64 {
        match c {
            0 => 0,
            _ => (a as u128).checked_mul(b as u128).unwrap()
                            .checked_div(c as u128).unwrap() as u64
        }
    }

    pub fn calculate_output_amount_for_buying_asset(
        current_amount: u64,
        target_amount: u64,
        pyth: SimplePrice,
        amount_value: u64,
        prism_data: TokenPriceData,
        decimals: u8,
    ) -> u64 {
        let curve_start_amount = if current_amount < target_amount
            { target_amount } else { current_amount };
    
        let mut amount_value_left: u64 = amount_value;
        let mut current_output_amount: u64 = 0;
    
        let expo: u64 = u64::pow(10, decimals as u32);
        let mut pyth_price: u64 = SymmetryTokenSwap::usd_value(
            u64::pow(10, decimals as u32),
            decimals as u64,
            pyth,
        );
        pyth_price = SymmetryTokenSwap::mul_div(pyth_price, 1000000 + 5, 1000000);
        let mut current_price = pyth_price;
    
        let mut amount_from_target_weight: u64 = 0;
        for step in 0..10 {
            let price_in_interval = (prism_data.price[step] * 9 + pyth_price) / 10;
            if price_in_interval > current_price {
                current_price = price_in_interval;
            }
            amount_from_target_weight += prism_data.amount[step];
            if amount_from_target_weight <= curve_start_amount - current_amount {
                continue;
            }
    
            let amount_in_interval = std::cmp::min(
                amount_from_target_weight - (curve_start_amount - current_amount),
                prism_data.amount[step]
            );
            let value_in_interval = SymmetryTokenSwap::mul_div(amount_in_interval, current_price, expo);
            if value_in_interval > amount_value_left {
                 return SymmetryTokenSwap::mul_div(amount_value_left, expo, current_price) + current_output_amount;
            }
            current_output_amount += amount_in_interval;
            amount_value_left -= value_in_interval;
        }
        current_output_amount += SymmetryTokenSwap::mul_div(amount_value_left, expo, current_price);
        current_output_amount
    }

    pub fn calculate_output_value_for_selling_asset(
        current_amount: u64,
        target_amount: u64,
        pyth: SimplePrice,
        amount: u64,
        prism_data: TokenPriceData,
        decimals: u8,
    ) -> u64 {
        let curve_start_amount = if current_amount > target_amount
            { target_amount } else { current_amount };
    
        let mut current_output_value: u64 = 0;
        let mut amount_left: u64 = amount;
    
        let expo: u64 = u64::pow(10, decimals as u32);
        let mut pyth_price = SymmetryTokenSwap::usd_value(
            u64::pow(10, decimals as u32),
            decimals as u64,
            pyth
        );
        pyth_price = SymmetryTokenSwap::mul_div(pyth_price, 1000000 - 5, 1000000);
        let mut current_price = pyth_price;
    
        let mut amount_from_target_weight: u64 = 0;
    
        for step in 0..10 {
            let price_in_interval = (prism_data.price[step] * 9 + pyth_price) / 10;
            if price_in_interval < current_price {
                current_price = price_in_interval;
            }
            amount_from_target_weight += prism_data.amount[step];
            if amount_from_target_weight <= current_amount - curve_start_amount {
                continue;
            }
            let amount_in_interval = std::cmp::min(
                amount_from_target_weight - (current_amount - curve_start_amount),
                prism_data.amount[step],
            );
            let value_in_interval = SymmetryTokenSwap::mul_div(amount_in_interval, current_price, expo);
    
            if amount_in_interval > amount_left {
                 return SymmetryTokenSwap::mul_div(amount_left, current_price, expo) + current_output_value;
            }
            current_output_value += value_in_interval;
            amount_left -= amount_in_interval;
        }
        current_output_value += SymmetryTokenSwap::mul_div(amount_left, current_price, expo);
    
        current_output_value
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
            vec.push(self.token_info.token_mint[self.fund_state.current_comp_token[i] as usize])
        }
        return vec;
    }

    fn get_accounts_to_update(&self) -> Vec<Pubkey> {
        let mut accounts_to_update: Vec<Pubkey> = Vec::new();
        accounts_to_update.push(SymmetryTokenSwap::CURVE_DATA_ADDRESS);
        accounts_to_update.push(self.key);
        for i in 0..MAX_TOKENS_IN_ASSET_POOL {
            if self.token_info.oracle[i] != Pubkey::default() {
                accounts_to_update.push(self.token_info.oracle[i])
            }
        }
        return accounts_to_update;
    }

    fn update(&mut self, accounts_map: &HashMap<Pubkey, Vec<u8>>) -> Result<()> {
        self.curve_data = CurveData::load(accounts_map.get(&SymmetryTokenSwap::CURVE_DATA_ADDRESS).unwrap());
        self.fund_state = FundState::load(accounts_map.get(&self.key).unwrap());
        for i in 0..50 {
            if self.token_info.oracle[i] != Pubkey::default() {
                self.token_info.oracle_price[i] = SimplePrice::load(accounts_map.get(&self.token_info.oracle[i]).unwrap());
            }
        }

        Ok(())
    }

    fn quote(&self, quote_params: &QuoteParams) -> Result<Quote> {
        
        let from_amount: u64 = quote_params.in_amount;
        let from_token_id: u64 = self.token_info.token_mint.iter().position(|&x| x == quote_params.input_mint).unwrap() as u64;
        let to_token_id: u64 = self.token_info.token_mint.iter().position(|&x| x == quote_params.output_mint).unwrap() as u64;
        
        let from_token_index: usize = self.fund_state.current_comp_token.iter()
                            .position(|&x| x == (from_token_id as u64)).unwrap() as usize;
        let to_token_index: usize = self.fund_state.current_comp_token.iter()
                            .position(|&x| x == (to_token_id as u64)).unwrap() as usize;

        let mut fund_worth = 0;
        for i in 0..(self.fund_state.num_of_tokens as usize) {
            let token = self.fund_state.current_comp_token[i] as usize;
            fund_worth += SymmetryTokenSwap::usd_value(
                self.fund_state.current_comp_amount[i],
                self.token_info.decimals[token] as u64,
                self.token_info.oracle_price[token],
            );
        }

        let from_token_price = self.token_info.oracle_price[from_token_id as usize];
        let to_token_price= self.token_info.oracle_price[to_token_id as usize];
        
        let from_token_target_amount: u64 = SymmetryTokenSwap::amount_from_usd_value(
            SymmetryTokenSwap::mul_div(self.fund_state.target_weight[from_token_index], fund_worth, self.fund_state.weight_sum),
            self.token_info.decimals[from_token_id as usize] as u64,
            from_token_price,
        );
        let to_token_target_amount: u64 = SymmetryTokenSwap::amount_from_usd_value(
            SymmetryTokenSwap::mul_div(self.fund_state.target_weight[to_token_index], fund_worth, self.fund_state.weight_sum),
            self.token_info.decimals[to_token_id as usize] as u64,
            to_token_price,
        );

        let from_token_value = SymmetryTokenSwap::usd_value(
            from_amount,
            self.token_info.decimals[from_token_id as usize] as u64,
            from_token_price,
        );

        let value = match from_token_id as usize {
            0 => from_token_value,
            _ => SymmetryTokenSwap::calculate_output_value_for_selling_asset(
                self.fund_state.current_comp_amount[from_token_index],
                from_token_target_amount,
                from_token_price,
                from_amount,
                self.curve_data.sell[from_token_id as usize],
                self.token_info.decimals[from_token_id as usize],
            ),
        };

        let mut to_amount = match to_token_id as usize {
            0 => SymmetryTokenSwap::amount_from_usd_value(
                value,
                self.token_info.decimals[to_token_id as usize] as u64,
                to_token_price,
            ),
            _ => SymmetryTokenSwap::calculate_output_amount_for_buying_asset(
                self.fund_state.current_comp_amount[to_token_index],
                to_token_target_amount,
                to_token_price,
                value,
                self.curve_data.buy[to_token_id as usize],
                self.token_info.decimals[to_token_id as usize],
            ),
        };

        let mut value_without_curve = from_token_value;
        if from_token_id != 0 as u64 {
            value_without_curve = SymmetryTokenSwap::mul_div(value_without_curve, 1000000 - 5, 1000000)
        }
        let mut amount_without_curve = SymmetryTokenSwap::amount_from_usd_value(
            value_without_curve,
            self.token_info.decimals[to_token_id as usize] as u64,
            to_token_price,
        );
        if to_token_id != 0 as u64 {
            amount_without_curve = SymmetryTokenSwap::mul_div(amount_without_curve, 1000000 - 5, 1000000);
        }
        
        let mut fee_due_nel: u64 = 0;
        if amount_without_curve > self.fund_state.current_comp_amount[to_token_index] {
            fee_due_nel = amount_without_curve - self.fund_state.current_comp_amount[to_token_index];
            amount_without_curve = self.fund_state.current_comp_amount[to_token_index];
        }

        if to_amount > amount_without_curve {
            to_amount = amount_without_curve
        }

        let total_fees = amount_without_curve - to_amount;
        let symmetry_fee = SymmetryTokenSwap::mul_div(total_fees, 5, 100);
        let host_fee = SymmetryTokenSwap::mul_div(total_fees, 20, 100);
        let manager_fee = SymmetryTokenSwap::mul_div(total_fees, 20, 100);
        let fund_fee = total_fees - symmetry_fee - host_fee - manager_fee;

        fund_worth = fund_worth - SymmetryTokenSwap::usd_value(
            self.fund_state.current_comp_amount[from_token_index],
            self.token_info.decimals[from_token_id as usize] as u64,
            from_token_price,
        );
        fund_worth = fund_worth - SymmetryTokenSwap::usd_value(
            self.fund_state.current_comp_amount[to_token_index],
            self.token_info.decimals[to_token_id as usize] as u64,
            to_token_price,
        );

        let from_token_worth_after_swap: u64 = SymmetryTokenSwap::usd_value(
            self.fund_state.current_comp_amount[from_token_index] + from_amount,
            self.token_info.decimals[from_token_id as usize] as u64,
            from_token_price,
        );
        let to_token_worth_after_swap: u64 = SymmetryTokenSwap::usd_value(
            self.fund_state.current_comp_amount[to_token_index] - amount_without_curve + fund_fee,
            self.token_info.decimals[to_token_id as usize] as u64,
            to_token_price,
        );
        fund_worth = fund_worth + from_token_worth_after_swap;
        fund_worth = fund_worth + to_token_worth_after_swap;

        let allowed_offset = (self.fund_state.rebalance_threshold * self.fund_state.lp_offset_threshold) as u128;

        let allowed_from_target_weight = 
            (self.fund_state.target_weight[from_token_index] as u128) *
            (100000000 + allowed_offset) / 100000000;

        if ((from_token_worth_after_swap as u128) * (self.fund_state.weight_sum as u128) >
            (allowed_from_target_weight) * (fund_worth as u128))
             && (from_token_id != 0 as u64)
              && (allowed_from_target_weight < 10000 as u128) {
            return Ok(Quote {
                not_enough_liquidity: true,
                out_amount: 0,
                ..Quote::default()
            })
        }

        let allowed_to_target_weight =
            (self.fund_state.target_weight[to_token_index] as u128) *
            (100000000 - allowed_offset) / 100000000;

        if (to_token_worth_after_swap as u128) * (self.fund_state.weight_sum as u128) <
            (allowed_to_target_weight) * (fund_worth as u128) {
                return Ok(Quote {
                    not_enough_liquidity: true,
                    out_amount: 0,
                    ..Quote::default()
                })
        }
        
        let all_fees = total_fees + fee_due_nel;
        let zero_slippage_price = amount_without_curve - fund_fee + all_fees;
        
        Ok(Quote {
            in_amount: quote_params.in_amount,
            out_amount: to_amount,
            fee_amount: all_fees,
            fee_mint: quote_params.output_mint,
            price_impact_pct: Decimal::new(SymmetryTokenSwap::mul_div(all_fees, 1000000, zero_slippage_price) as i64, 4),
            fee_pct: Decimal::new(SymmetryTokenSwap::mul_div(all_fees, 1000000, zero_slippage_price) as i64, 4),
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
        
        let from_token_id: u64 = self.token_info.token_mint.iter().position(|&x| x == *source_mint).unwrap() as u64;
        let to_token_id: u64 = self.token_info.token_mint.iter().position(|&x| x == *destination_mint).unwrap() as u64;

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
        account_metas.push(AccountMeta::new(self.token_info.pda_ta[from_token_id as usize], false));
        account_metas.push(AccountMeta::new(*user_source_token_account, false));
        account_metas.push(AccountMeta::new(self.token_info.pda_ta[to_token_id as usize], false));
        account_metas.push(AccountMeta::new(*user_destination_token_account, false));
        account_metas.push(AccountMeta::new(swap_to_fee, false));
        account_metas.push(AccountMeta::new(host_to_fee, false));
        account_metas.push(AccountMeta::new(manager_to_fee, false));
        account_metas.push(AccountMeta::new_readonly(SymmetryTokenSwap::TOKEN_INFO_ADDRESS, false));
        account_metas.push(AccountMeta::new_readonly(SymmetryTokenSwap::CURVE_DATA_ADDRESS, false));
        account_metas.push(AccountMeta::new_readonly(SymmetryTokenSwap::SPL_TOKEN_PROGRAM_ADDRESS, false));

        // Pyth Oracle accounts are being passed as remaining accounts
        for i in 0..self.fund_state.num_of_tokens as usize {
            account_metas.push(
                AccountMeta::new_readonly(self.token_info.oracle[self.fund_state.current_comp_token[i] as usize], false)
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
    const SOL_TOKEN_MINT: Pubkey = pubkey!("So11111111111111111111111111111111111111112");
    const USDC_TOKEN_MINT: Pubkey = pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");

    use crate::amms::test_harness::AmmTestHarness;

    /* Init Token Swap */
    const TOKEN_INFO_ACCOUNT: Pubkey = SymmetryTokenSwap::TOKEN_INFO_ADDRESS;
    const FUND_STATE_ACCOUNT: Pubkey = pubkey!("Db86JGJnM58KtcZjqf8JFn3md98TDWJZLJJFBzkEWccZ");

    let test_harness = AmmTestHarness::new();
    let fund_state_account = test_harness.get_keyed_account(FUND_STATE_ACCOUNT).unwrap();
    let token_info_account = test_harness.get_keyed_account(TOKEN_INFO_ACCOUNT).unwrap();
    let mut token_swap = SymmetryTokenSwap::from_keyed_account(&fund_state_account, &token_info_account).unwrap();

    /* Update TokenSwap (FundState + CurveData + Pyth Oracle accounts) */
    test_harness.update_amm(&mut token_swap);

    /* Token mints available for swap in a fund */
    println!("-------------------");
    let token_mints = token_swap.get_reserve_mints();
    println!("Available mints for swap: {:?}", token_mints);
    let from_token_mint: Pubkey = token_mints.clone().into_iter().find(|&x| x == SOL_TOKEN_MINT).unwrap();
    let to_token_mint: Pubkey = token_mints.clone().into_iter().find(|&x| x == USDC_TOKEN_MINT).unwrap(); 

    /* Get Quote */
    println!("-------------------");
    let in_amount: u64 = 1_000_000_000_000; // 1000 SOL -> ? USDC
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
