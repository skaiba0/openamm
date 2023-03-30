use crate::instructions::create_pool::POOL_SEED;
use crate::stableswap::{calc_d, calc_dy, get_token_decs_fac, STABLESWAP_AMP_COEFFICIENT};
use crate::state::*;
use anchor_lang::prelude::*;
use anchor_spl::dex;
use anchor_spl::token::{Token, TokenAccount};
use serum_dex::critbit::*;
use serum_dex::instruction::MarketInstruction;
use serum_dex::instruction::{CancelOrderInstructionV2, NewOrderInstructionV3, SelfTradeBehavior};
use serum_dex::matching::OrderType;
use serum_dex::matching::{OrderBookState, Side};
use serum_dex::state::Market;
use solana_program::instruction::{AccountMeta, Instruction};
use std::cmp;
use std::num::NonZeroU64;

const ORDER_NUMERATORS: [u16; 10] = [8, 15, 30, 50, 125, 300, 500, 750, 1000, 1250];

const LP_FEE_BPS: u16 = 20;
const STABLESWAP_FEE_BPS: u16 = 4;

pub fn get_orderbook<'info>(
    curr_client_order_id: u64,
    pool_bump: u8,
    pool_type: PoolType,
    pool: AccountLoader<'info, OpenAmmPool>,
    market_accounts: MarketAccounts<'info>,
    base_wallet: Account<'info, TokenAccount>,
    quote_wallet: Account<'info, TokenAccount>,
    dex_program: Program<'info, dex::Dex>,
    token_program: Program<'info, Token>,
    rent: Sysvar<'info, Rent>,
    should_print_orders: bool,
) -> OrderbookClient<'info> {
    let should_load_orders = true;
    let base_lot_size;
    let quote_lot_size;
    let mut native_base_total = 0;
    let mut native_quote_total = 0;
    let mut native_base_free = 0;
    let mut native_quote_free = 0;
    let mut best_bid_price = None;
    let mut best_ask_price = None;
    let mut orders = vec![];
    let should_load_price = false;
    let market = market_accounts.market.clone();
    let mut market_state = Market::load(&market, &dex::ID, true).unwrap();

    base_lot_size = market_state.coin_lot_size;
    quote_lot_size = market_state.pc_lot_size;

    if should_load_orders || should_load_price {
        let open_orders = Market::load_orders_mut(
            &market_state,
            &market_accounts.open_orders,
            None,
            &dex::ID,
            None,
            None,
        )
        .unwrap();

        native_base_total = open_orders.native_coin_total;
        native_quote_total = open_orders.native_pc_total;
        native_base_free = open_orders.native_coin_free;
        native_quote_free = open_orders.native_pc_free;

        let mut asks = market_state.load_asks_mut(&market_accounts.asks).unwrap();
        let mut bids = market_state.load_bids_mut(&market_accounts.bids).unwrap();
        let mut orderbook_state = OrderBookState {
            bids: &mut bids,
            asks: &mut asks,
            market_state: &mut market_state,
        };

        if should_load_price {
            let bid_id = orderbook_state.bids.find_max();
            let ask_id = orderbook_state.asks.find_min();
            if bid_id.is_some() && ask_id.is_some() {
                let best_bid = orderbook_state
                    .orders_mut(Side::Bid)
                    .get_mut(bid_id.unwrap())
                    .unwrap()
                    .as_leaf_mut()
                    .unwrap()
                    .clone();
                let best_ask = orderbook_state
                    .orders_mut(Side::Ask)
                    .get_mut(ask_id.unwrap())
                    .unwrap()
                    .as_leaf_mut()
                    .unwrap()
                    .clone();
                best_bid_price = u64::from(best_bid.price()).into();
                best_ask_price = u64::from(best_ask.price()).into();
            }
        }

        if should_load_orders {
            let max_orders: u64 = (ORDER_NUMERATORS.len() * 2).try_into().unwrap();

            let slots = open_orders.iter_filled_slots();
            for slot in slots {
                let c_id = NonZeroU64::new(open_orders.client_order_ids[slot as usize]).unwrap();

                if curr_client_order_id > max_orders {
                    let last_min_c_id =
                        NonZeroU64::new(curr_client_order_id.checked_sub(max_orders).unwrap())
                            .unwrap();
                    if c_id < last_min_c_id {
                        continue;
                    }
                }

                let order_id = open_orders.orders[slot as usize];
                let side = open_orders.slot_side(slot).unwrap();
                let order_handle = orderbook_state.orders_mut(side).find_by_key(order_id);
                if let Some(order_handle) = order_handle {
                    let order = orderbook_state
                        .orders_mut(side)
                        .get_mut(order_handle)
                        .unwrap()
                        .as_leaf_mut()
                        .unwrap();
                    let limit_price: u64 = order.price().into();
                    let base_qty: u64 = order.quantity().into();

                    if should_print_orders {
                        msg!("{:?} {} {}", side, limit_price, base_qty);
                    }

                    orders.push(CurrentOrder {
                        side,
                        order_id,
                        limit_price,
                        base_qty,
                        client_order_id: order.client_order_id(),
                    });
                }
            }
        }
    }
    drop(market_state);

    OrderbookClient {
        market_accounts,
        pool,
        pool_bump,
        pool_type,
        dex_program,
        token_program,
        rent,
        base_lot_size,
        quote_lot_size,
        orders,
        native_base_total,
        native_quote_total,
        native_base_free,
        native_quote_free,
        base_wallet,
        quote_wallet,
        best_bid_price,
        best_ask_price,
    }
}

#[derive(Clone)]
pub struct OrderbookClient<'info> {
    pub market_accounts: MarketAccounts<'info>,
    pub base_wallet: Account<'info, TokenAccount>,
    pub quote_wallet: Account<'info, TokenAccount>,
    pub dex_program: Program<'info, dex::Dex>,
    pub token_program: Program<'info, Token>,
    pub pool: AccountLoader<'info, OpenAmmPool>,
    pub rent: Sysvar<'info, Rent>,
    pub base_lot_size: u64,
    pub quote_lot_size: u64,
    pub native_base_total: u64,
    pub native_quote_total: u64,
    pub native_base_free: u64,
    pub native_quote_free: u64,
    pub orders: Vec<CurrentOrder>,
    pub best_bid_price: Option<u64>,
    pub best_ask_price: Option<u64>,
    pub pool_bump: u8,
    pub pool_type: PoolType,
}

impl<'info> OrderbookClient<'info> {
    pub fn place_orders(
        &self,
        place_ixs: Vec<NewOrderInstructionV3>,
        ask_payer: AccountInfo<'info>,
        bid_payer: AccountInfo<'info>,
    ) -> Result<()> {
        let accounts = vec![
            AccountMeta::new(self.market_accounts.market.key(), false),
            AccountMeta::new(self.market_accounts.open_orders.key(), false),
            AccountMeta::new(self.market_accounts.request_queue.key(), false),
            AccountMeta::new(self.market_accounts.event_queue.key(), false),
            AccountMeta::new(self.market_accounts.bids.key(), false),
            AccountMeta::new(self.market_accounts.asks.key(), false),
            AccountMeta::new(ask_payer.key(), false),
            AccountMeta::new_readonly(self.pool.key(), true),
            AccountMeta::new(self.market_accounts.base_vault.key(), false),
            AccountMeta::new(self.market_accounts.quote_vault.key(), false),
            AccountMeta::new_readonly(self.token_program.key(), false),
            AccountMeta::new_readonly(self.rent.key(), false),
        ];
        let mut account_infos = vec![
            self.dex_program.to_account_info(),
            self.market_accounts.market.clone(),
            self.market_accounts.open_orders.clone(),
            self.market_accounts.request_queue.clone(),
            self.market_accounts.event_queue.clone(),
            self.market_accounts.bids.clone(),
            self.market_accounts.asks.clone(),
            ask_payer.clone(),
            self.pool.to_account_info(),
            self.market_accounts.base_vault.to_account_info(),
            self.market_accounts.quote_vault.to_account_info(),
            self.token_program.to_account_info(),
            self.rent.to_account_info(),
        ];

        let mut instruction = Instruction {
            program_id: self.dex_program.key(),
            data: vec![],
            accounts,
        };

        let market_key = self.market_accounts.market.key();
        let pool_type_bytes = (self.pool_type as u8).to_le_bytes();
        let seeds = pool_authority_seeds!(
            market_key = market_key,
            pool_type_bytes = pool_type_bytes,
            bump = self.pool_bump
        );
        let pool_signer = &[&seeds[..]];

        for place in place_ixs.iter() {
            let new_order_ix = MarketInstruction::NewOrderV3(place.clone());
            match place.side {
                serum_dex::matching::Side::Ask => {
                    instruction.accounts[6] = AccountMeta::new(ask_payer.key(), false);
                    account_infos[7] = ask_payer.to_account_info();
                }
                _ => {
                    instruction.accounts[6] = AccountMeta::new(bid_payer.key(), false);
                    account_infos[7] = bid_payer.to_account_info();
                }
            };
            instruction.data = new_order_ix.pack();
            solana_program::program::invoke_signed(&instruction, &account_infos, pool_signer)?;
        }

        Ok(())
    }

    pub fn cancel_orders(&self, cancel_ixs: Vec<CancelOrderInstructionV2>) -> Result<()> {
        let mut instruction = Instruction {
            program_id: self.dex_program.key(),
            data: vec![],
            accounts: vec![
                AccountMeta::new(self.market_accounts.market.key(), false),
                AccountMeta::new(self.market_accounts.bids.key(), false),
                AccountMeta::new(self.market_accounts.asks.key(), false),
                AccountMeta::new(self.market_accounts.open_orders.key(), false),
                AccountMeta::new_readonly(self.pool.key(), true),
                AccountMeta::new(self.market_accounts.event_queue.key(), false),
            ],
        };

        let account_infos = [
            self.dex_program.to_account_info(),
            self.market_accounts.market.clone(),
            self.market_accounts.bids.clone(),
            self.market_accounts.asks.clone(),
            self.market_accounts.open_orders.clone(),
            self.pool.to_account_info(),
            self.market_accounts.event_queue.clone(),
        ];

        let market_key = self.market_accounts.market.key();
        let pool_type_bytes = (self.pool_type as u8).to_le_bytes();
        let seeds = pool_authority_seeds!(
            market_key = market_key,
            pool_type_bytes = pool_type_bytes,
            bump = self.pool_bump
        );
        let pool_signer = &[&seeds[..]];

        for cancel in cancel_ixs.iter() {
            let cancel_instruction = MarketInstruction::CancelOrderV2(cancel.clone());
            instruction.data = cancel_instruction.pack();
            solana_program::program::invoke_signed(&instruction, &account_infos, pool_signer).ok();
        }

        Ok(())
    }

    pub fn cancel_all_and_settle(&self) -> Result<()> {
        const REFUND_DENOMINATOR: u16 = 10_000;
        let mut pool = self.pool.load_mut().unwrap();

        let curr_asks = self
            .orders
            .iter()
            .filter(|o| o.side == Side::Ask)
            .cloned()
            .collect::<Vec<CurrentOrder>>();

        let curr_bids = self
            .orders
            .iter()
            .filter(|o| o.side == Side::Bid)
            .cloned()
            .collect::<Vec<CurrentOrder>>();

        let non_zero_asks = pool
            .placed_asks
            .iter()
            .filter(|o| o.base_qty != 0)
            .cloned()
            .collect::<Vec<PlacedOrder>>();

        let non_zero_bids = pool
            .placed_bids
            .iter()
            .filter(|o| o.base_qty != 0)
            .cloned()
            .collect::<Vec<PlacedOrder>>();

        let mut moved_base_amount: u64 = 0;
        let mut moved_quote_amount: u64 = 0;

        for (i, placed_ask) in non_zero_asks.iter().enumerate() {
            let placed_base_amount = placed_ask.base_qty.checked_mul(self.base_lot_size).unwrap();
            let found_curr_ask = curr_asks
                .iter()
                .find(|&&o| o.client_order_id == placed_ask.client_order_id);

            let less_base_amount = if let Some(found_curr_ask) = found_curr_ask {
                let curr_base_amount = found_curr_ask
                    .base_qty
                    .checked_mul(self.base_lot_size)
                    .unwrap();

                placed_base_amount.checked_sub(curr_base_amount).unwrap()
            }
            else {
                if i == non_zero_asks.len() - 1 {
                    pool.mm_active = false;
                }
                placed_base_amount
            };

            let more_quote_amount = less_base_amount
                .checked_mul(placed_ask.limit_price)
                .unwrap()
                .checked_mul(self.quote_lot_size)
                .unwrap()
                .checked_div(self.base_lot_size)
                .unwrap();

            let refund_amount = more_quote_amount
                .checked_div(REFUND_DENOMINATOR.into())
                .unwrap();

            pool.base_amount = pool.base_amount.checked_sub(less_base_amount).unwrap();
            pool.quote_amount = pool
                .quote_amount
                .checked_add(more_quote_amount)
                .unwrap()
                .checked_sub(refund_amount)
                .unwrap();

            moved_quote_amount = moved_quote_amount.checked_add(more_quote_amount).unwrap();
            pool.cumulative_quote_volume = pool
                .cumulative_quote_volume
                .checked_add(more_quote_amount)
                .unwrap();
        }

        for (i, placed_bid) in non_zero_bids.iter().enumerate() {
            let max_base_qty = placed_bid
                .max_native_quote_qty_including_fees
                .checked_div(placed_bid.limit_price)
                .unwrap();

            let base_qty = cmp::min(max_base_qty, placed_bid.base_qty);
            let placed_base_amount = base_qty.checked_mul(self.base_lot_size).unwrap();

            let found_curr_bid = curr_bids
                .iter()
                .find(|&&o| o.client_order_id == placed_bid.client_order_id);

            let more_base_amount = if let Some(found_curr_bid) = found_curr_bid {
                let curr_base_amount = found_curr_bid
                    .base_qty
                    .checked_mul(self.base_lot_size)
                    .unwrap();
                placed_base_amount.checked_sub(curr_base_amount).unwrap()
            }
            else {
                if i == non_zero_bids.len() - 1 {
                    pool.mm_active = false
                }
                placed_base_amount
            };

            let less_quote_amount = more_base_amount
                .checked_mul(placed_bid.limit_price)
                .unwrap()
                .checked_mul(self.quote_lot_size)
                .unwrap()
                .checked_div(self.base_lot_size)
                .unwrap();

            let refund_amount = more_base_amount
                .checked_div(REFUND_DENOMINATOR.into())
                .unwrap();

            moved_base_amount = moved_base_amount.checked_add(more_base_amount).unwrap();

            pool.base_amount = pool
                .base_amount
                .checked_add(more_base_amount)
                .unwrap()
                .checked_sub(refund_amount)
                .unwrap();
            pool.quote_amount = pool.quote_amount.checked_sub(less_quote_amount).unwrap();
            pool.cumulative_base_volume = pool
                .cumulative_base_volume
                .checked_add(more_base_amount)
                .unwrap();
        }

        let mut cancel_ixs = vec![];
        for order in self.orders.iter() {
            let cancel_ix = CancelOrderInstructionV2 {
                side: order.side,
                order_id: order.order_id,
            };
            cancel_ixs.push(cancel_ix);
        }

        pool.reset_placed_orders();

        pool.refund_quote_amount = pool
            .refund_quote_amount
            .checked_add(
                moved_quote_amount
                    .checked_div(REFUND_DENOMINATOR.into())
                    .unwrap(),
            )
            .unwrap();
        pool.refund_base_amount = pool
            .refund_base_amount
            .checked_add(
                moved_base_amount
                    .checked_div(REFUND_DENOMINATOR.into())
                    .unwrap(),
            )
            .unwrap();

        drop(pool);
        self.cancel_orders(cancel_ixs)?;

        self.settle()?;

        Ok(())
    }

    pub fn settle(&self) -> Result<()> {
        let settle_accs = dex::SettleFunds {
            market: self.market_accounts.market.clone(),
            open_orders: self.market_accounts.open_orders.clone(),
            open_orders_authority: self.pool.to_account_info(),
            coin_vault: self.market_accounts.base_vault.to_account_info(),
            pc_vault: self.market_accounts.quote_vault.to_account_info(),
            coin_wallet: self.base_wallet.to_account_info(),
            pc_wallet: self.quote_wallet.to_account_info(),
            vault_signer: self.market_accounts.vault_signer.clone(),
            token_program: self.token_program.to_account_info(),
        };
        let market_key = self.market_accounts.market.key();
        let pool_type_bytes = (self.pool_type as u8).to_le_bytes();
        let seeds = pool_authority_seeds!(
            market_key = market_key,
            pool_type_bytes = pool_type_bytes,
            bump = self.pool_bump
        );
        let pool_signer = &[&seeds[..]];

        let ctx = CpiContext::new_with_signer(
            self.dex_program.to_account_info(),
            settle_accs,
            pool_signer,
        );
        dex::settle_funds(ctx)
    }

    pub fn place_new_orders(
        &self,
        base_vault: &Account<'info, TokenAccount>,
        quote_vault: &Account<'info, TokenAccount>,
    ) -> Result<()> {
        let mut pool_loader = self.pool.load_init();
        if pool_loader.is_err() {
            pool_loader = self.pool.load_mut();
        }
        let pool = pool_loader?;
        match pool.pool_type {
            PoolType::XYK => {
                drop(pool);
                self.place_xyk_orders(base_vault, quote_vault)
            }
            PoolType::STABLE => {
                drop(pool);
                self.place_stableswap_orders(base_vault, quote_vault)
            }
        }
    }

    fn place_stableswap_orders(
        &self,
        pool_base_vault: &Account<'info, TokenAccount>,
        pool_quote_vault: &Account<'info, TokenAccount>,
    ) -> Result<()> {
        let mut pool_loader = self.pool.load_init();
        if pool_loader.is_err() {
            pool_loader = self.pool.load_mut();
        }
        let mut pool = pool_loader?;
        const FEE_DENOMINATOR: u16 = 10_000;
        const ORDER_DENOMINATOR: u16 = 10_000;

        let base_reserve = pool.base_amount;
        let quote_reserve = pool.quote_amount;

        let (base_decs_fac, quote_decs_fac) =
            get_token_decs_fac(pool.base_decimals, pool.quote_decimals);

        let (base_reserve, quote_reserve) = (
            base_reserve.checked_mul(base_decs_fac).unwrap(),
            quote_reserve.checked_mul(quote_decs_fac).unwrap(),
        );

        if base_reserve == 0 || quote_reserve == 0 {
            return Ok(());
        }

        let ask_fee_numerator = FEE_DENOMINATOR
            .checked_add(STABLESWAP_FEE_BPS.into())
            .unwrap();

        let bid_fee_numerator = (FEE_DENOMINATOR)
            .checked_sub(STABLESWAP_FEE_BPS.into())
            .unwrap();

        let mut place_ixs = vec![];

        let OrderbookClient {
            best_bid_price,
            best_ask_price,
            ..
        } = self;

        let mut last_ask_base = base_reserve;
        let mut last_ask_quote = quote_reserve;
        let mut last_bid_base = base_reserve;
        let mut last_bid_quote = quote_reserve;

        let d = calc_d(last_ask_base, last_ask_quote, STABLESWAP_AMP_COEFFICIENT).unwrap();

        for i in 0..ORDER_NUMERATORS.len() {
            let a_size: u64 = (base_reserve as u128)
                .checked_mul(ORDER_NUMERATORS[i].into())
                .unwrap()
                .checked_div(ORDER_DENOMINATOR.into())
                .unwrap()
                .try_into()
                .unwrap();
            let end_a_amount = last_ask_base.checked_sub(a_size).unwrap_or(0);

            if end_a_amount > 0 && a_size > 0 {
                let b_size = calc_dy(
                    last_ask_base,
                    last_ask_quote,
                    STABLESWAP_AMP_COEFFICIENT,
                    d,
                    a_size,
                )
                .unwrap_or(0);
                let end_b_amount = last_ask_quote + b_size;

                let (a_size, b_size) = (a_size / base_decs_fac, b_size / quote_decs_fac);

                let a_lots = a_size.checked_div(self.base_lot_size).unwrap();

                let mut limit_price: u64 = (b_size as u128)
                    .checked_mul(ask_fee_numerator.into())
                    .unwrap()
                    .checked_mul(self.base_lot_size.into())
                    .unwrap()
                    .checked_div(a_size.into())
                    .unwrap()
                    .checked_div(FEE_DENOMINATOR.into())
                    .unwrap()
                    .checked_div(self.quote_lot_size.into())
                    .unwrap()
                    .try_into()
                    .unwrap();

                last_ask_base = end_a_amount;
                last_ask_quote = end_b_amount;

                if limit_price != 0 && a_lots != 0 && b_size != 0 {
                    if best_bid_price.is_some() && limit_price <= best_bid_price.unwrap() {
                        limit_price = best_bid_price.unwrap().checked_add(1).unwrap();
                    }

                    let client_order_id = pool.client_order_id;
                    let place_ix = NewOrderInstructionV3 {
                        side: Side::Ask,
                        limit_price: NonZeroU64::new(limit_price).unwrap(),
                        max_coin_qty: NonZeroU64::new(a_lots).unwrap(),
                        max_native_pc_qty_including_fees: NonZeroU64::new(b_size).unwrap(),
                        self_trade_behavior: SelfTradeBehavior::DecrementTake,
                        order_type: OrderType::PostOnly,
                        client_order_id: pool.client_order_id,
                        limit: 0,
                        max_ts: i64::MAX,
                    };
                    pool.placed_asks[i] = PlacedOrder {
                        limit_price: place_ix.limit_price.into(),
                        base_qty: place_ix.max_coin_qty.into(),
                        max_native_quote_qty_including_fees: place_ix
                            .max_native_pc_qty_including_fees
                            .into(),
                        client_order_id,
                    };

                    place_ixs.push(place_ix);
                    pool.client_order_id += 1;
                }
            }
        }

        for i in 0..ORDER_NUMERATORS.len() - 1 {
            let b_size: u64 = (quote_reserve as u128)
                .checked_mul(ORDER_NUMERATORS[i].into())
                .unwrap()
                .checked_div(ORDER_DENOMINATOR.into())
                .unwrap()
                .try_into()
                .unwrap();

            let end_b_amount = last_bid_quote.checked_sub(b_size).unwrap_or_else(|| 0);

            if end_b_amount > 0 && b_size > 0 {
                let a_size = calc_dy(
                    last_bid_quote,
                    last_bid_base,
                    STABLESWAP_AMP_COEFFICIENT,
                    d,
                    b_size,
                )
                .unwrap_or(0);
                let end_a_amount = last_bid_base + a_size;

                let (a_size, b_size) = (a_size / base_decs_fac, b_size / quote_decs_fac);

                let a_lots = a_size.checked_div(self.base_lot_size).unwrap();

                let mut limit_price: u64 = (b_size as u128)
                    .checked_mul(bid_fee_numerator.into())
                    .unwrap()
                    .checked_mul(self.base_lot_size.into())
                    .unwrap()
                    .checked_div(a_size.into())
                    .unwrap()
                    .checked_div(FEE_DENOMINATOR.into())
                    .unwrap()
                    .checked_div(self.quote_lot_size.into())
                    .unwrap()
                    .try_into()
                    .unwrap();

                last_bid_base = end_a_amount;
                last_bid_quote = end_b_amount;

                if limit_price != 0 && a_lots != 0 && b_size != 0 {
                    if best_ask_price.is_some()
                        && limit_price >= best_ask_price.unwrap()
                        && best_ask_price.unwrap() > 1
                    {
                        limit_price = best_ask_price.unwrap().checked_sub(1).unwrap();
                    }

                    let client_order_id = pool.client_order_id;
                    let place_ix = NewOrderInstructionV3 {
                        side: Side::Bid,
                        limit_price: NonZeroU64::new(limit_price).unwrap(),
                        max_coin_qty: NonZeroU64::new(a_lots).unwrap(),
                        max_native_pc_qty_including_fees: NonZeroU64::new(b_size).unwrap(),
                        self_trade_behavior: SelfTradeBehavior::DecrementTake,
                        order_type: OrderType::PostOnly,
                        client_order_id,
                        limit: 0,
                        max_ts: i64::MAX,
                    };
                    pool.placed_bids[i] = PlacedOrder {
                        limit_price: place_ix.limit_price.into(),
                        base_qty: place_ix.max_coin_qty.into(),
                        max_native_quote_qty_including_fees: place_ix
                            .max_native_pc_qty_including_fees
                            .into(),
                        client_order_id,
                    };

                    place_ixs.push(place_ix);
                    pool.client_order_id += 1;
                }
            }
        }
        drop(pool);

        self.place_orders(
            place_ixs,
            pool_base_vault.to_account_info(),
            pool_quote_vault.to_account_info(),
        )
        .unwrap();
        Ok(())
    }

    fn place_xyk_orders(
        &self,
        pool_base_vault: &Account<'info, TokenAccount>,
        pool_quote_vault: &Account<'info, TokenAccount>,
    ) -> Result<()> {
        let mut pool_loader = self.pool.load_init();
        if pool_loader.is_err() {
            pool_loader = self.pool.load_mut();
        }
        let mut pool = pool_loader?;
        const FEE_DENOMINATOR: u16 = 10_000;
        const ORDER_DENOMINATOR: u16 = 10_000;

        let ask_fee_numerator = FEE_DENOMINATOR.checked_add(LP_FEE_BPS.into()).unwrap();

        let bid_fee_numerator = (FEE_DENOMINATOR).checked_sub(LP_FEE_BPS.into()).unwrap();

        let base_reserve = pool.base_amount;
        let quote_reserve = pool.quote_amount;

        if base_reserve == 0 || quote_reserve == 0 {
            return Ok(());
        }

        let mut place_ixs = vec![];

        let OrderbookClient {
            best_bid_price,
            best_ask_price,
            ..
        } = self;

        let mut last_ask_base = base_reserve;
        let mut last_ask_quote = quote_reserve;
        let mut last_bid_base = base_reserve;
        let mut last_bid_quote = quote_reserve;

        for i in 0..ORDER_NUMERATORS.len() {
            let a_size: u64 = (base_reserve as u128)
                .checked_mul(ORDER_NUMERATORS[i].into())
                .unwrap()
                .checked_div(ORDER_DENOMINATOR.into())
                .unwrap()
                .try_into()
                .unwrap();
            let k = (last_ask_base as u128)
                .checked_mul(last_ask_quote.into())
                .unwrap();
            let end_a_amount = last_ask_base.checked_sub(a_size).unwrap_or_else(|| 0);

            if end_a_amount > 0 {
                let end_b_amount: u64 = k
                    .checked_div(end_a_amount.into())
                    .unwrap()
                    .try_into()
                    .unwrap();
                let delta_b = end_b_amount.checked_sub(last_ask_quote).unwrap();
                let b_size = delta_b;
                let a_lots = a_size.checked_div(self.base_lot_size).unwrap();

                let mut limit_price: u64 = (delta_b as u128)
                    .checked_mul(self.base_lot_size.into())
                    .unwrap()
                    .checked_mul(ask_fee_numerator.into())
                    .unwrap()
                    .checked_div(a_size.into())
                    .unwrap()
                    .checked_div(self.quote_lot_size.into())
                    .unwrap()
                    .checked_div(FEE_DENOMINATOR.into())
                    .unwrap()
                    .try_into()
                    .unwrap();

                last_ask_base = end_a_amount;
                last_ask_quote = end_b_amount;

                if limit_price != 0 && a_lots != 0 && b_size != 0 {
                    if best_bid_price.is_some() && limit_price <= best_bid_price.unwrap() {
                        limit_price = best_bid_price.unwrap().checked_add(1).unwrap();
                    }

                    let client_order_id = pool.client_order_id;
                    let place_ix = NewOrderInstructionV3 {
                        side: Side::Ask,
                        limit_price: NonZeroU64::new(limit_price).unwrap(),
                        max_coin_qty: NonZeroU64::new(a_lots).unwrap(),
                        max_native_pc_qty_including_fees: NonZeroU64::new(b_size).unwrap(),
                        self_trade_behavior: SelfTradeBehavior::DecrementTake,
                        order_type: OrderType::PostOnly,
                        client_order_id,
                        limit: 0,
                        max_ts: i64::MAX,
                    };
                    pool.placed_asks[i] = PlacedOrder {
                        limit_price: place_ix.limit_price.into(),
                        base_qty: place_ix.max_coin_qty.into(),
                        max_native_quote_qty_including_fees: place_ix
                            .max_native_pc_qty_including_fees
                            .into(),
                        client_order_id,
                    };

                    place_ixs.push(place_ix);
                    pool.client_order_id += 1;
                }
            }
        }

        for i in 0..ORDER_NUMERATORS.len() - 1 {
            let b_size: u64 = (quote_reserve as u128)
                .checked_mul(ORDER_NUMERATORS[i].into())
                .unwrap()
                .checked_div(ORDER_DENOMINATOR.into())
                .unwrap()
                .try_into()
                .unwrap();
            let k = (last_bid_base as u128)
                .checked_mul(last_bid_quote.into())
                .unwrap();
            let end_b_amount = last_bid_quote.checked_sub(b_size).unwrap_or_else(|| 0);

            if end_b_amount > 0 {
                let end_a_amount: u64 = k
                    .checked_div(end_b_amount.into())
                    .unwrap()
                    .try_into()
                    .unwrap();
                let delta_a = end_a_amount.checked_sub(last_bid_base).unwrap();
                let a_size = delta_a;
                let a_lots = a_size.checked_div(self.base_lot_size).unwrap();
                let mut limit_price: u64 = (b_size as u128)
                    .checked_mul(self.base_lot_size.into())
                    .unwrap()
                    .checked_mul(bid_fee_numerator.into())
                    .unwrap()
                    .checked_div(delta_a.into())
                    .unwrap()
                    .checked_div(self.quote_lot_size.into())
                    .unwrap()
                    .checked_div(FEE_DENOMINATOR.into())
                    .unwrap()
                    .try_into()
                    .unwrap();

                last_bid_base = end_a_amount;
                last_bid_quote = end_b_amount;

                if limit_price != 0 && a_lots != 0 && b_size != 0 {
                    if best_ask_price.is_some()
                        && limit_price >= best_ask_price.unwrap()
                        && best_ask_price.unwrap() > 1
                    {
                        limit_price = best_ask_price.unwrap().checked_sub(1).unwrap();
                    }

                    let place_ix = NewOrderInstructionV3 {
                        side: Side::Bid,
                        limit_price: NonZeroU64::new(limit_price).unwrap(),
                        max_coin_qty: NonZeroU64::new(a_lots).unwrap(),
                        max_native_pc_qty_including_fees: NonZeroU64::new(b_size).unwrap(),
                        self_trade_behavior: SelfTradeBehavior::DecrementTake,
                        order_type: OrderType::PostOnly,
                        client_order_id: pool.client_order_id,
                        limit: 0,
                        max_ts: i64::MAX,
                    };

                    pool.placed_bids[i] = PlacedOrder {
                        limit_price: place_ix.limit_price.into(),
                        base_qty: place_ix.max_coin_qty.into(),
                        max_native_quote_qty_including_fees: place_ix
                            .max_native_pc_qty_including_fees
                            .into(),
                        client_order_id: pool.client_order_id,
                    };

                    place_ixs.push(place_ix);
                    pool.client_order_id += 1;
                }
            }
        }
        drop(pool);

        self.place_orders(
            place_ixs,
            pool_base_vault.to_account_info(),
            pool_quote_vault.to_account_info(),
        )
        .unwrap();
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub struct CurrentOrder {
    pub side: Side,
    pub order_id: u128,
    pub client_order_id: u64,
    pub limit_price: u64,
    pub base_qty: u64,
}

pub fn same_fraction(fraction1: (u64, u64), fraction2: (u64, u64)) -> bool {
    let gcd1 = gcd(fraction1.0, fraction1.1);
    let gcd2 = gcd(fraction2.0, fraction2.1);

    let reduced_fraction1 = (fraction1.0 / gcd1, fraction1.1 / gcd1);
    let reduced_fraction2 = (fraction2.0 / gcd2, fraction2.1 / gcd2);

    reduced_fraction1 == reduced_fraction2
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

macro_rules! pool_authority_seeds {
    (
        market_key = $market_key:expr,
        pool_type_bytes = $pool_type_bytes:expr,
        bump = $bump:expr
    ) => {
        &[
            $market_key.as_ref(),
            $pool_type_bytes.as_ref(),
            POOL_SEED.as_bytes(),
            &[$bump],
        ]
    };
}

pub(crate) use pool_authority_seeds;

macro_rules! init {
    ($zeroed_item:ident = $Struct:ident {
        $($field:ident: $value:expr),*$(,)?
    } $(ignoring {
        $($ignored_field:ident),*$(,)?
    })?) => {
        $($zeroed_item.$field = $value;)*
        #[allow(unreachable_code)]
        if false {
            let _ = $Struct {
                $($field: $value,)*
                $($($ignored_field: panic!("fix the bug in `init`"),)*)?
            };
        }
    };
}
pub(crate) use init;
