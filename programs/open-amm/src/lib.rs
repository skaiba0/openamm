use anchor_lang::prelude::*;

declare_id!("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS");

pub mod errors;
pub mod instructions;
pub mod state;

use instructions::*;
use state::*;
pub(crate) mod stableswap;
pub(crate) mod util;

#[program]
pub mod open_amm {
    use super::*;

    pub fn create_pool<'info>(
        ctx: Context<'_, '_, '_, 'info, CreatePool<'info>>,
        pool_type: PoolType,
        initial_base_amount: u64,
        initial_quote_amount: u64,
    ) -> Result<()> {
        return instructions::create_pool::handler(
            ctx,
            pool_type,
            initial_base_amount,
            initial_quote_amount,
        );
    }

    pub fn deposit<'info>(
        ctx: Context<'_, '_, '_, 'info, Deposit<'info>>,
        desired_base_amount: u64,
        desired_quote_amount: u64,
        min_base_amount: u64,
        min_quote_amount: u64,
    ) -> Result<()> {
        return instructions::deposit::handler(
            ctx,
            desired_base_amount,
            desired_quote_amount,
            min_base_amount,
            min_quote_amount,
        );
    }

    pub fn withdraw<'info>(
        ctx: Context<'_, '_, '_, 'info, Withdraw<'info>>,
        lp_amt: u64,
    ) -> Result<()> {
        return instructions::withdraw::handler(ctx, lp_amt);
    }

    pub fn refresh_orders<'info>(
        ctx: Context<'_, '_, '_, 'info, RefreshOrders<'info>>,
    ) -> Result<()> {
        return instructions::refresh_orders::handler(ctx);
    }

    pub fn restart_market_making<'info>(
        ctx: Context<'_, '_, '_, 'info, RestartMarketMaking<'info>>,
    ) -> Result<()> {
        return instructions::restart_market_making::handler(ctx);
    }
}
