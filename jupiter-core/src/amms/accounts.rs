use anchor_lang::prelude::*;
use std::convert::TryInto;

pub const MAX_TOKENS_IN_ASSET_POOL: usize = 100;
pub const NUM_TOKENS_IN_FUND: usize = 20;
pub const NUM_OF_POINTS_IN_CURVE_DATA: usize = 10;
pub const ONE_USD: u64 = 1_000_000_000_000;
pub const USE_CURVE_DATA: u8 = 1;
pub const BPS_DIVIDER: u64 = 10000;
pub const WEIGHT_MULTIPLIER: u64 = 10000;
pub const LP_DISABLED: u8 = 0;

pub fn mul_div(a: u64, b: u64, c: u64) -> u64 {
    match c {
        0 => 0,
        _ => ((a as u128) * (b as u128) / (c as u128)) as u64
    }
}

#[derive(Clone, Copy)]
pub struct FundState {
    pub manager: Pubkey,
    pub host_pubkey: Pubkey,
    pub num_of_tokens: u64,
    pub current_comp_token: [u64; NUM_TOKENS_IN_FUND],
    pub current_comp_amount: [u64; NUM_TOKENS_IN_FUND],
    pub target_weight: [u64; NUM_TOKENS_IN_FUND],
    pub weight_sum: u64,
    pub rebalance_threshold: u64,
    pub lp_offset_threshold: u64,
}

impl FundState {
    #[inline]
    pub fn load<'a>(account_data: &Vec<u8>) -> FundState {
        let mut current_comp_token: [u64; NUM_TOKENS_IN_FUND] = [0 as u64; NUM_TOKENS_IN_FUND];
        let mut current_comp_amount: [u64; NUM_TOKENS_IN_FUND] = [0 as u64; NUM_TOKENS_IN_FUND];
        let mut target_weight: [u64; NUM_TOKENS_IN_FUND] = [0 as u64; NUM_TOKENS_IN_FUND];
        for i in 0..NUM_TOKENS_IN_FUND {
            current_comp_token[i] = u64::from_le_bytes(account_data[(176 + i*8)..(184 + i*8)].try_into().unwrap());
            current_comp_amount[i] = u64::from_le_bytes(account_data[(336 + i*8)..(344 + i*8)].try_into().unwrap());
            target_weight[i] = u64::from_le_bytes(account_data[(656 + i*8)..(664 + i*8)].try_into().unwrap());
        }
        let num_of_tokens = u64::from_le_bytes(account_data[168..176].try_into().unwrap());
        let weight_sum = u64::from_le_bytes(account_data[816..824].try_into().unwrap());
        let rebalance_threshold = u64::from_le_bytes(account_data[1024..1032].try_into().unwrap());
        let lp_offset_threshold = u64::from_le_bytes(account_data[1040..1048].try_into().unwrap());
        FundState {
            manager: Pubkey::new_from_array(account_data[16..48].try_into().unwrap()),
            host_pubkey: Pubkey::new_from_array(account_data[128..160].try_into().unwrap()),
            num_of_tokens,
            current_comp_token,
            current_comp_amount,
            target_weight,
            weight_sum,
            rebalance_threshold,
            lp_offset_threshold,
        }
    }
}

#[derive(Clone, Copy)]
pub struct TokenSettings {                                      // 199 bytes
    pub token_mint: Pubkey,                                     // 32 bytes
    pub decimals: u8,                                           // 1 byte
    pub coingecko_id: [u8; 30],                                 // 30 bytes
    pub pda_token_account: Pubkey,                              // 32 bytes
    pub oracle_type: u8,                                        // 1 byte
    pub oracle_account: Pubkey,                                 // 32 bytes
    pub oracle_index: u8,                                       // 1 byte
    pub oracle_confidence_pct: u8,                              // 1 byte
    pub fixed_confidence_bps: u8,                               // 1 byte
    pub token_swap_fee_after_tw_bps: u8,                        // 1 byte
    pub token_swap_fee_before_tw_bps: u8,                       // 1 byte
    pub is_live: u8,                                            // 1 byte
    pub lp_on: u8,                                              // 1 byte
    pub use_curve_data: u8,                                     // 1 byte
    pub additional_data: [u8; 63],                              // 64 bytes
    pub oracle_price: OraclePrice,
}

#[derive(Clone, Copy)]
pub struct TokenList {                                          // 39808 bytes
    pub num_tokens: u64,                                        // 8 bytes
    pub list: [TokenSettings; MAX_TOKENS_IN_ASSET_POOL],        // 39800 bytes
}

impl TokenList {
    #[inline]
    pub fn load<'a>(account_data: &Vec<u8>) -> TokenList {
        let num_tokens = u64::from_le_bytes(account_data[8..16].try_into().unwrap());
        let mut list: [TokenSettings; MAX_TOKENS_IN_ASSET_POOL] = [
            TokenSettings {
                token_mint: Pubkey::default(),
                decimals: 0,
                coingecko_id: [0; 30],
                pda_token_account: Pubkey::default(),
                oracle_type: 0,
                oracle_account: Pubkey::default(),
                oracle_index: 0,
                oracle_confidence_pct: 0,
                fixed_confidence_bps: 0,
                token_swap_fee_after_tw_bps: 0,
                token_swap_fee_before_tw_bps: 0,
                is_live: 0,
                lp_on: 0,
                use_curve_data: 0,
                additional_data: [0; 63],
                oracle_price: OraclePrice { sell_price: 0, avg_price: 0, buy_price: 0, oracle_live: 0}
            };
            MAX_TOKENS_IN_ASSET_POOL
        ];
        for i in 0..num_tokens as usize {
            let slice: [u8; 199] = account_data[16 + i*199..16 + (i+1)*199].try_into().unwrap();
            list[i].token_mint = Pubkey::new_from_array(slice[0..32].try_into().unwrap());
            list[i].decimals = slice[32];
            list[i].pda_token_account = Pubkey::new_from_array(slice[63..95].try_into().unwrap());
            list[i].oracle_type = slice[95];
            list[i].oracle_account = Pubkey::new_from_array(slice[96..128].try_into().unwrap());
            list[i].oracle_index = slice[128];
            list[i].oracle_confidence_pct = slice[129];
            list[i].fixed_confidence_bps = slice[130];
            list[i].token_swap_fee_after_tw_bps = slice[131];
            list[i].token_swap_fee_before_tw_bps = slice[132];
            list[i].is_live = slice[133];
            list[i].lp_on = slice[134];
            list[i].use_curve_data = slice[135];
            list[i].additional_data = slice[136..199].try_into().unwrap();
        }
        TokenList { num_tokens, list, }
    }
}


#[derive(PartialEq, Debug, Copy, Clone)]
#[repr(C)]
pub struct TokenPriceData {
    pub amount: [u64; NUM_OF_POINTS_IN_CURVE_DATA],
    pub price: [u64; NUM_OF_POINTS_IN_CURVE_DATA],
}

#[derive(Clone, Copy)]
pub struct CurveData {
    pub buy: [TokenPriceData; MAX_TOKENS_IN_ASSET_POOL],
    pub sell: [TokenPriceData; MAX_TOKENS_IN_ASSET_POOL],
}

impl CurveData {
    #[inline]
    pub fn load<'a>(account_data: &Vec<u8>) -> CurveData {
        let mut buy_vec: Vec<TokenPriceData> = Vec::new();
        let mut sell_vec: Vec<TokenPriceData> = Vec::new();
        for _ in 0..MAX_TOKENS_IN_ASSET_POOL {
            buy_vec.push(TokenPriceData {
                amount: [0; NUM_OF_POINTS_IN_CURVE_DATA],
                price: [0; NUM_OF_POINTS_IN_CURVE_DATA],
            });
            sell_vec.push(TokenPriceData {
                amount: [0; NUM_OF_POINTS_IN_CURVE_DATA],
                price: [0; NUM_OF_POINTS_IN_CURVE_DATA],
            });
        }
        let mut buy: [TokenPriceData; MAX_TOKENS_IN_ASSET_POOL] = buy_vec.try_into().unwrap();
        let mut sell: [TokenPriceData; MAX_TOKENS_IN_ASSET_POOL] = sell_vec.try_into().unwrap();
        for i in 0..MAX_TOKENS_IN_ASSET_POOL {
            for j in 0..NUM_OF_POINTS_IN_CURVE_DATA {
                buy[i].amount[j] = u64::from_le_bytes(account_data[(8 + i*160 + j*8)..(16 + i*160 + j*8)].try_into().unwrap());
                buy[i].price[j] = u64::from_le_bytes(account_data[(88 + i*160 + j*8)..(96 + i*160 + j*8)].try_into().unwrap());
                sell[i].amount[j] = u64::from_le_bytes(account_data[(32008 + i*160 + j*8)..(32016 + i*160 + j*8)].try_into().unwrap());
                sell[i].price[j] = u64::from_le_bytes(account_data[(32088 + i*160 + j*8)..(32096 + i*160 + j*8)].try_into().unwrap());
            }
        }
        CurveData {
            buy,
            sell,
        }
    }

    pub fn empty() -> CurveData {
        let mut buy_vec: Vec<TokenPriceData> = Vec::new();
        let mut sell_vec: Vec<TokenPriceData> = Vec::new();
        for _ in 0..MAX_TOKENS_IN_ASSET_POOL {
            buy_vec.push(TokenPriceData {
                amount: [0; NUM_OF_POINTS_IN_CURVE_DATA],
                price: [0; NUM_OF_POINTS_IN_CURVE_DATA],
            });
            sell_vec.push(TokenPriceData {
                amount: [0; NUM_OF_POINTS_IN_CURVE_DATA],
                price: [0; NUM_OF_POINTS_IN_CURVE_DATA],
            });
        }
        let buy: [TokenPriceData; MAX_TOKENS_IN_ASSET_POOL] = buy_vec.try_into().unwrap();
        let sell: [TokenPriceData; MAX_TOKENS_IN_ASSET_POOL] = sell_vec.try_into().unwrap();
        CurveData {
            buy,
            sell,
        }
    }
}

#[derive(Clone, Copy)]
pub struct OraclePrice {
    pub sell_price: u64,
    pub avg_price: u64,
    pub buy_price: u64,
    pub oracle_live: u8,
}

impl OraclePrice {
    #[inline]
    pub fn load<'a>(account_data: &Vec<u8>, token_settings: TokenSettings) -> OraclePrice {

        let (price, coinfidence, oracle_live) = match token_settings.oracle_type {
            0 => {
                let valid_slot: u64 =  u64::from_le_bytes(account_data[40..48].try_into().unwrap());
                let expo: i32 = i32::from_le_bytes(account_data[20..24].try_into().unwrap());
                let price: i64 =  i64::from_le_bytes(account_data[208..216].try_into().unwrap());
                let conf: u64 = u64::from_le_bytes(account_data[216..224].try_into().unwrap());
                let status: u32 = u32::from_le_bytes(account_data[224..228].try_into().unwrap());
                let mut oracle_live = 1;
        
                if Clock::get().unwrap_or_default().slot >= 50 + valid_slot {
                    oracle_live = 0;
                }
                if status != 1 {
                    oracle_live = 0;
                }
                if price < 0 {
                    oracle_live = 0;
                }
                if conf * 10 > price as u64 {
                    oracle_live = 0;
                }
                
                let pow_num = u64::pow(10, (-expo) as u32);
                let avg_price = mul_div(price as u64, ONE_USD, pow_num);
                let confidence = mul_div(conf, ONE_USD, pow_num);
    
                let base_confidene = mul_div(
                    confidence, 
                    token_settings.oracle_confidence_pct as u64, 
                    100
                );
                
                (avg_price, base_confidene, oracle_live)
            },
            1 => {
                
                let price_start = (token_settings.oracle_index as usize) * 8 + 9;
                let price_end = price_start + 8;
                let price: [u8; 8] = account_data[price_start..price_end].try_into().unwrap();
                let mantissa: u64 = u64::from_le_bytes(price);
    
                let timestamp_start = price_start + 400;
                let timetamp_end = price_end + 400;
                let t: [u8; 8] = account_data[timestamp_start..timetamp_end].try_into().unwrap();
                let write_timestamp: u64 = u64::from_le_bytes(t);
                let mut oracle_live: u8 = 1;
                
                if Clock::get().unwrap_or_default().unix_timestamp as u64 > write_timestamp + 15 {
                    oracle_live = 0;
                }

                let base_confidence = mul_div(
                    mantissa,
                    token_settings.oracle_confidence_pct as u64,
                    10000
                );
                
                (mantissa - base_confidence, base_confidence, oracle_live)
            }
            _ => (0, 0, 0)
        };
    
        let additional_confidence = mul_div(
            price,
            token_settings.fixed_confidence_bps as u64,
            10000
        );
    
        OraclePrice {
            sell_price: price - coinfidence - additional_confidence,
            avg_price: price,
            buy_price: price + coinfidence + additional_confidence,
            oracle_live: oracle_live,
        }
    }
}
