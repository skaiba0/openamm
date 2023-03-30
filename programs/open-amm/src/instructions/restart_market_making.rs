use crate::errors::OpenAmmErrorCode;
use crate::state::*;
use crate::util::get_orderbook;
use anchor_lang::prelude::*;
use anchor_spl::dex;
use anchor_spl::token::{Token, TokenAccount};

#[derive(Accounts)]
pub struct RestartMarketMaking<'info> {
    #[account(
        mut,
        has_one = base_vault,
        has_one = quote_vault,
    )]
    pub pool: AccountLoader<'info, OpenAmmPool>,

    #[account(
        constraint = market_accounts.market.key() == pool.load()?.market 
            @ OpenAmmErrorCode::WrongMarketAccount,
        constraint = market_accounts.open_orders.key() == pool.load()?.open_orders
            @ OpenAmmErrorCode::WrongOpenOrdersAccount,
    )]
    pub market_accounts: MarketAccounts<'info>,

    #[account(mut)]
    pub base_vault: Box<Account<'info, TokenAccount>>,

    #[account(mut)]
    pub quote_vault: Box<Account<'info, TokenAccount>>,

    #[account(mut)]
    pub signer: Signer<'info>,

    pub token_program: Program<'info, Token>,

    #[account(address = dex::ID)]
    pub dex_program: Program<'info, dex::Dex>,

    pub rent: Sysvar<'info, Rent>,
}

/**
 * This is here in case the placed ask was not actually fully filled, but
 * openamm recieved a bunch of orders and pushed our order off of the book
 * because the data structure was full
 * In this case, we want to pause market making until we are sure
 * that the order is actually filled.
 */
pub fn handler<'info>(ctx: Context<'_, '_, '_, 'info, RestartMarketMaking<'info>>) -> Result<()> {
    let pool_bump = ctx.bumps.get("pool").unwrap().clone();
    let pool = ctx.accounts.pool.load()?;
    let order_id = pool.client_order_id;
    let pool_type = pool.pool_type;
    require!(!pool.mm_active, OpenAmmErrorCode::MarketMakingAlreadyActive);
    drop(pool);

    let orderbook = get_orderbook(
        order_id,
        pool_bump,
        pool_type,
        ctx.accounts.pool.clone(),
        ctx.accounts.market_accounts.clone(),
        *ctx.accounts.base_vault.clone(),
        *ctx.accounts.quote_vault.clone(),
        ctx.accounts.dex_program.clone(),
        ctx.accounts.token_program.clone(),
        ctx.accounts.rent.clone(),
        false,
    );

    orderbook.cancel_all_and_settle()?;

    require!(
        orderbook.native_base_total == 0 && orderbook.native_quote_total == 0,
        OpenAmmErrorCode::OpenOrdersTokensLocked,
    );

    let mut pool = ctx.accounts.pool.load_mut()?;
    pool.base_amount = ctx.accounts.base_vault.amount;
    pool.quote_amount = ctx.accounts.quote_vault.amount;
    pool.mm_active = true;
    Ok(())
}
