use anchor_lang::prelude::*;
use num_derive::{FromPrimitive, ToPrimitive};

#[derive(AnchorSerialize, Default, AnchorDeserialize, Copy, Clone, FromPrimitive, ToPrimitive)]
pub enum PoolType {
    #[default]
    XYK = 0,
    STABLE = 1,
}

#[zero_copy]
#[derive(Default)]
pub struct PlacedOrder {
    pub limit_price: u64,
    pub base_qty: u64,
    pub max_native_quote_qty_including_fees: u64,
    pub client_order_id: u64,
}

#[account(zero_copy)]
pub struct OpenAmmPool {
    pub base_amount: u64,
    pub quote_amount: u64,
    pub cumulative_quote_volume: u64,
    pub cumulative_base_volume: u64,
    pub refund_base_amount: u64,
    pub refund_quote_amount: u64,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub market: Pubkey,
    pub open_orders: Pubkey,
    pub base_vault: Pubkey,
    pub quote_vault: Pubkey,
    pub lp_mint: Pubkey,
    pub client_order_id: u64,
    pub pool_type: PoolType,
    pub base_decimals: u8,
    pub quote_decimals: u8,
    pub bump: u8,
    pub placed_asks: [PlacedOrder; 10],
    pub placed_bids: [PlacedOrder; 10],
    pub mm_active: bool,
}

impl OpenAmmPool {
    pub fn reset_placed_orders(&mut self) -> () {
        self.placed_asks = [PlacedOrder::default(); 10];
        self.placed_bids = [PlacedOrder::default(); 10];
    }
}
