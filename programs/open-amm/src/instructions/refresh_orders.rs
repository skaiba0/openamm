use crate::errors::OpenAmmErrorCode;
use crate::instructions::create_pool::POOL_SEED;
use crate::state::*;
use crate::util::{get_orderbook, pool_authority_seeds};
use anchor_lang::prelude::*;
use anchor_spl::dex;
use anchor_spl::token::{transfer, Token, TokenAccount, Transfer};

#[derive(Accounts)]
pub struct RefreshOrders<'info> {
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

    #[account(
        mut,
        token::mint = base_vault.mint,
        token::authority = signer,
    )]
    pub signer_base: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = quote_vault.mint,
        token::authority = signer,
    )]
    pub signer_quote: Box<Account<'info, TokenAccount>>,

    #[account(mut)]
    pub signer: Signer<'info>,

    pub token_program: Program<'info, Token>,

    #[account(address = dex::ID)]
    pub dex_program: Program<'info, dex::Dex>,

    pub rent: Sysvar<'info, Rent>,
}

pub fn handler<'info>(ctx: Context<'_, '_, '_, 'info, RefreshOrders<'info>>) -> Result<()> {
    let pool = ctx.accounts.pool.load()?;
    let pool_bump = pool.bump;
    let order_id = pool.client_order_id;
    let pool_type = pool.pool_type;
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

    let pool = ctx.accounts.pool.load()?;
    if !pool.mm_active {
        return Ok(());
    }
    drop(pool);

    orderbook.place_new_orders(&ctx.accounts.base_vault, &ctx.accounts.quote_vault)?;

    let mut pool = ctx.accounts.pool.load_mut()?;
    let refund_quote_amount = pool.refund_quote_amount;
    let refund_base_amount = pool.refund_base_amount;
    pool.refund_quote_amount = 0;
    pool.refund_base_amount = 0;
    drop(pool);

    let market_key = ctx.accounts.market_accounts.market.key();
    let pool_type_bytes = (pool_type as u8).to_le_bytes();
    let seeds = pool_authority_seeds!(
        market_key = market_key,
        pool_type_bytes = pool_type_bytes,
        bump = pool_bump
    );
    let pool_signer = &[&seeds[..]];

    let cpi_token_program = ctx.accounts.token_program.to_account_info();
    let transfer_base_to_signer_cpi_ctx = CpiContext::new_with_signer(
        cpi_token_program.clone(),
        Transfer {
            from: ctx.accounts.base_vault.to_account_info(),
            to: ctx.accounts.signer_base.to_account_info(),
            authority: ctx.accounts.pool.to_account_info(),
        },
        pool_signer,
    );
    transfer(transfer_base_to_signer_cpi_ctx, refund_base_amount)?;

    let transfer_quote_to_signer_cpi_ctx = CpiContext::new_with_signer(
        cpi_token_program,
        Transfer {
            from: ctx.accounts.quote_vault.to_account_info(),
            to: ctx.accounts.signer_quote.to_account_info(),
            authority: ctx.accounts.pool.to_account_info(),
        },
        pool_signer,
    );
    transfer(transfer_quote_to_signer_cpi_ctx, refund_quote_amount)?;
    Ok(())
}
