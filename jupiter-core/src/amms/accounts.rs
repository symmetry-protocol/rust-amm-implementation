use anchor_lang::prelude::*;
use std::convert::TryInto;

pub const MAX_TOKENS_IN_ASSET_POOL: usize = 50;
pub const NUM_TOKENS_IN_FUND: usize = 20;
pub const NUM_OF_POINTS_IN_CURVE_DATA: usize = 10;

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

pub struct TokenInfo {
    pub token_mint: [Pubkey; MAX_TOKENS_IN_ASSET_POOL],
    pub pda_ta: [Pubkey; MAX_TOKENS_IN_ASSET_POOL],
    pub oracle: [Pubkey; MAX_TOKENS_IN_ASSET_POOL],
    pub decimals: [u8; MAX_TOKENS_IN_ASSET_POOL],
    pub oracle_price: [SimplePrice; MAX_TOKENS_IN_ASSET_POOL],
}

impl TokenInfo {
    #[inline]
    pub fn load<'a>(account_data: &Vec<u8>) -> TokenInfo {
        let mut token_mint: [Pubkey; MAX_TOKENS_IN_ASSET_POOL] = [Pubkey::default(); MAX_TOKENS_IN_ASSET_POOL];
        let mut oracle: [Pubkey; MAX_TOKENS_IN_ASSET_POOL] = [Pubkey::default(); MAX_TOKENS_IN_ASSET_POOL];
        let mut pda_ta: [Pubkey; MAX_TOKENS_IN_ASSET_POOL] = [Pubkey::default(); MAX_TOKENS_IN_ASSET_POOL];
        let mut decimals: [u8; MAX_TOKENS_IN_ASSET_POOL] = [0 as u8; MAX_TOKENS_IN_ASSET_POOL];
        let mut oracle_price: Vec<SimplePrice> = Vec::new();
        for i in 0..MAX_TOKENS_IN_ASSET_POOL {
            token_mint[i] = Pubkey::new_from_array(account_data[(16 + i*32)..(48 + i*32)].try_into().unwrap());
            pda_ta[i] = Pubkey::new_from_array(account_data[(6416 + i*32)..(6448 + i*32)].try_into().unwrap());
            oracle[i] = Pubkey::new_from_array(account_data[(18816 + i*32)..(18848 + i*32)].try_into().unwrap());
            decimals[i] = account_data[25216 + i];
            oracle_price.push(SimplePrice { expo: 0, price: 0, low: 0, high: 0 });
        }
        TokenInfo {
            token_mint,
            pda_ta,
            oracle,
            decimals,
            oracle_price: oracle_price.try_into().unwrap(),
        }
    }
}


#[derive(PartialEq, Debug, Copy, Clone)]
#[repr(C)]
pub struct TokenPriceData {
    pub amount: [u64; NUM_OF_POINTS_IN_CURVE_DATA],
    pub price: [u64; NUM_OF_POINTS_IN_CURVE_DATA],
}

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
        let mut buy: [TokenPriceData; 50] = buy_vec.try_into().unwrap();
        let mut sell: [TokenPriceData; 50] = sell_vec.try_into().unwrap();
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

#[derive(PartialEq, Debug, Copy, Clone)]
#[repr(C)]
pub struct SimplePrice {
    pub expo: i32,
    pub price: i64,
    pub low: i64,
    pub high: i64,
}

impl SimplePrice {
    #[inline]
    pub fn load<'a>(account_data: &Vec<u8>) -> SimplePrice {
        let expo: i32 = i32::from_le_bytes(account_data[20..24].try_into().unwrap());
        let price: i64 =  i64::from_le_bytes(account_data[208..216].try_into().unwrap());
        let conf: u128 = u64::from_le_bytes(account_data[216..224].try_into().unwrap()) as u128;
        let low: i64 = ((price as u128 * (100000 - 1)) / 100000 - conf / 2) as i64;
        let high: i64 = ((price as u128 * (100000 + 1)) / 100000 + conf / 2) as i64;
        SimplePrice {
            expo: expo,
            price: price,
            low: low,
            high: high,
        }
    }
}
