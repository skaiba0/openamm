use crate::errors::OpenAmmErrorCode;
use crate::instructions::create_pool::{LP_MINT_SEED, POOL_SEED};
use crate::state::*;
use crate::util::{get_orderbook, pool_authority_seeds};
use anchor_lang::prelude::*;
use anchor_spl::dex;
use anchor_spl::token::{burn, transfer, Burn, Mint, Token, TokenAccount, Transfer};

#[event]
pub struct WithdrawEvent {
    pool_type: PoolType,
    start_base: u64,
    start_quote: u64,
    start_lp: u64,
    end_base: u64,
    end_quote: u64,
    end_lp: u64,
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(
        mut,
        has_one = base_vault,
        has_one = quote_vault,
        has_one = lp_mint,
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
        mint::authority = pool,
        seeds = [pool.key().as_ref(), LP_MINT_SEED.as_bytes().as_ref()],
        bump,
    )]
    pub lp_mint: Box<Account<'info, Mint>>,

    #[account(
        mut,
        token::authority = signer,
        token::mint = base_vault.mint,
    )]
    pub signer_base: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        token::authority = signer,
        token::mint = quote_vault.mint,
    )]
    pub signer_quote: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        token::authority = signer,
        token::mint = lp_mint.key(),
    )]
    pub signer_lp: Box<Account<'info, TokenAccount>>,

    #[account(mut)]
    pub signer: Signer<'info>,

    pub token_program: Program<'info, Token>,

    #[account(address = dex::ID)]
    pub dex_program: Program<'info, dex::Dex>,

    pub rent: Sysvar<'info, Rent>,
}

pub fn handler<'info>(ctx: Context<'_, '_, '_, 'info, Withdraw<'info>>, lp_amt: u64) -> Result<()> {
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

    let mut pool = ctx.accounts.pool.load_mut()?;
    if !pool.mm_active {
        return Ok(());
    }
    let cpi_token_program = ctx.accounts.token_program.to_account_info();
    let base_reserve = pool.base_amount;
    let quote_reserve = pool.quote_amount;
    let start_lp = ctx.accounts.lp_mint.supply;

    let burn_lp_cpi_ctx = CpiContext::new(
        cpi_token_program.clone(),
        Burn {
            mint: ctx.accounts.lp_mint.to_account_info(),
            from: ctx.accounts.signer_lp.to_account_info(),
            authority: ctx.accounts.signer.to_account_info(),
        },
    );
    burn(burn_lp_cpi_ctx, lp_amt)?;

    let withdraw_base_amount: u64 = (lp_amt as u128)
        .checked_mul(base_reserve.into())
        .unwrap()
        .checked_div(start_lp.into())
        .unwrap()
        .try_into()
        .unwrap();

    let withdraw_quote_amount: u64 = (lp_amt as u128)
        .checked_mul(quote_reserve.into())
        .unwrap()
        .checked_div(start_lp.into())
        .unwrap()
        .try_into()
        .unwrap();

    let market_key = ctx.accounts.market_accounts.market.key();
    let pool_type_bytes = (pool_type as u8).to_le_bytes();
    let seeds = pool_authority_seeds!(
        market_key = market_key,
        pool_type_bytes = pool_type_bytes,
        bump = pool_bump
    );
    let pool_signer = &[&seeds[..]];

    pool.base_amount = pool.base_amount.checked_sub(withdraw_base_amount).unwrap();
    pool.quote_amount = pool
        .quote_amount
        .checked_sub(withdraw_quote_amount)
        .unwrap();

    drop(pool);
    let transfer_base_to_signer_cpi_ctx = CpiContext::new_with_signer(
        cpi_token_program.clone(),
        Transfer {
            from: ctx.accounts.base_vault.to_account_info(),
            to: ctx.accounts.signer_base.to_account_info(),
            authority: ctx.accounts.pool.to_account_info(),
        },
        pool_signer,
    );
    transfer(transfer_base_to_signer_cpi_ctx, withdraw_base_amount)?;

    let transfer_quote_to_signer_cpi_ctx = CpiContext::new_with_signer(
        cpi_token_program,
        Transfer {
            from: ctx.accounts.quote_vault.to_account_info(),
            to: ctx.accounts.signer_quote.to_account_info(),
            authority: ctx.accounts.pool.to_account_info(),
        },
        pool_signer,
    );
    transfer(transfer_quote_to_signer_cpi_ctx, withdraw_quote_amount)?;

    orderbook.place_new_orders(&ctx.accounts.base_vault, &ctx.accounts.quote_vault)?;

    let pool = ctx.accounts.pool.load()?;
    emit!(WithdrawEvent {
        pool_type: pool.pool_type,
        start_base: base_reserve,
        start_quote: quote_reserve,
        start_lp,
        end_base: pool.base_amount,
        end_quote: pool.quote_amount,
        end_lp: ctx.accounts.lp_mint.supply,
    });

    Ok(())
}
