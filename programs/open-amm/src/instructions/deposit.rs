use crate::errors::OpenAmmErrorCode;
use crate::instructions::create_pool::{LP_MINT_SEED, MINIMUM_LIQUIDITY, POOL_SEED};
use crate::stableswap::calculate_stableswap_lp_minted;
use crate::state::*;
use crate::util::{get_orderbook, pool_authority_seeds, same_fraction};
use anchor_lang::prelude::*;
use anchor_spl::dex;
use anchor_spl::token::{mint_to, transfer, Mint, MintTo, Token, TokenAccount, Transfer};
use std::cmp;
use std::mem::drop;

#[event]
pub struct DepositEvent {
    pool_type: PoolType,
    start_base: u64,
    start_quote: u64,
    start_lp: u64,
    end_base: u64,
    end_quote: u64,
    end_lp: u64,
}

#[derive(Accounts)]
pub struct Deposit<'info> {
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
        seeds = [pool.key().as_ref(), LP_MINT_SEED.as_bytes().as_ref()],
        bump,
        mint::authority = pool,
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
        token::mint = lp_mint,
        token::authority = signer,
    )]
    pub signer_lp: Box<Account<'info, TokenAccount>>,

    #[account(mut)]
    pub signer: Signer<'info>,

    pub token_program: Program<'info, Token>,

    #[account(address = dex::ID)]
    pub dex_program: Program<'info, dex::Dex>,

    pub rent: Sysvar<'info, Rent>,
}

pub fn handler<'info>(
    ctx: Context<'_, '_, '_, 'info, Deposit<'info>>,
    desired_base_amount: u64,
    desired_quote_amount: u64,
    min_base_amount: u64,
    min_quote_amount: u64,
) -> Result<()> {
    let cpi_token_program = ctx.accounts.token_program.to_account_info().clone();
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

    let reserve_base_amount = pool.base_amount;
    let reserve_quote_amount = pool.quote_amount;
    let start_lp = ctx.accounts.lp_mint.supply;
    let mut deposit_base_amount = desired_base_amount;
    let mut deposit_quote_amount = desired_quote_amount;

    if reserve_base_amount != 0 && reserve_quote_amount != 0 {
        if !same_fraction(
            (desired_quote_amount, desired_base_amount),
            (reserve_quote_amount, reserve_base_amount),
        ) {
            let optimal_quote_amount: u64 = (desired_base_amount as u128)
                .checked_mul(reserve_quote_amount.into())
                .unwrap()
                .checked_div(reserve_base_amount.into())
                .unwrap()
                .try_into()
                .unwrap();
            if optimal_quote_amount <= desired_quote_amount {
                require!(
                    optimal_quote_amount >= min_quote_amount,
                    OpenAmmErrorCode::SlippageQuoteExceeded
                );
                deposit_quote_amount = optimal_quote_amount;
            } else {
                let optimal_base_amount: u64 = (desired_quote_amount as u128)
                    .checked_mul(reserve_base_amount.into())
                    .unwrap()
                    .checked_div(reserve_quote_amount.into())
                    .unwrap()
                    .try_into()
                    .unwrap();
                require!(
                    optimal_base_amount <= desired_base_amount
                        && optimal_base_amount >= min_base_amount,
                    OpenAmmErrorCode::SlippageBaseExceeded,
                );
                deposit_base_amount = optimal_base_amount;
            }
        }
        let transfer_base_to_pool_cpi_ctx = CpiContext::new(
            cpi_token_program.clone(),
            Transfer {
                from: ctx.accounts.signer_base.to_account_info(),
                to: ctx.accounts.base_vault.to_account_info(),
                authority: ctx.accounts.signer.to_account_info(),
            },
        );
        transfer(transfer_base_to_pool_cpi_ctx, deposit_base_amount)?;
        pool.base_amount = pool.base_amount.checked_add(deposit_base_amount).unwrap();

        let transfer_quote_to_pool_cpi_ctx = CpiContext::new(
            cpi_token_program.clone(),
            Transfer {
                from: ctx.accounts.signer_quote.to_account_info(),
                to: ctx.accounts.quote_vault.to_account_info(),
                authority: ctx.accounts.signer.to_account_info(),
            },
        );
        transfer(transfer_quote_to_pool_cpi_ctx, deposit_quote_amount)?;

        pool.quote_amount = pool.quote_amount.checked_add(deposit_quote_amount).unwrap();
    }

    let lp_mint_supply = ctx.accounts.lp_mint.supply;
    let lp_minted: u64 = match pool.pool_type {
        PoolType::XYK => match lp_mint_supply {
            0 => ((deposit_base_amount as u128)
                .checked_mul(deposit_quote_amount as u128)
                .unwrap()
                .checked_sub(MINIMUM_LIQUIDITY.into())
                .unwrap() as f64)
                .sqrt() as u64,
            lp_mint_supply => cmp::min(
                (lp_mint_supply as u128)
                    .checked_mul(deposit_base_amount.into())
                    .unwrap()
                    .checked_div(reserve_base_amount.into())
                    .unwrap()
                    .try_into()
                    .unwrap(),
                (lp_mint_supply as u128)
                    .checked_mul(deposit_quote_amount.into())
                    .unwrap()
                    .checked_div(reserve_quote_amount.into())
                    .unwrap()
                    .try_into()
                    .unwrap(),
            ),
        },
        PoolType::STABLE => calculate_stableswap_lp_minted(
            lp_mint_supply,
            reserve_base_amount,
            reserve_quote_amount,
            deposit_base_amount,
            deposit_quote_amount,
            pool.base_decimals,
            pool.quote_decimals,
        ),
    };
    drop(pool);

    orderbook.place_new_orders(&ctx.accounts.base_vault, &ctx.accounts.quote_vault)?;

    let market_key = ctx.accounts.market_accounts.market.key();
    let pool_type_bytes = (pool_type as u8).to_le_bytes();
    let seeds = pool_authority_seeds!(
        market_key = market_key,
        pool_type_bytes = pool_type_bytes,
        bump = pool_bump
    );
    let pool_signer = &[&seeds[..]];

    let lp_mint_cpi_ctx = CpiContext::new_with_signer(
        cpi_token_program,
        MintTo {
            mint: ctx.accounts.lp_mint.to_account_info(),
            to: ctx.accounts.signer_lp.to_account_info(),
            authority: ctx.accounts.pool.to_account_info(),
        },
        pool_signer,
    );

    mint_to(lp_mint_cpi_ctx, lp_minted)?;

    let pool = ctx.accounts.pool.load()?;
    emit!(DepositEvent {
        pool_type: pool.pool_type,
        start_base: reserve_base_amount,
        start_quote: reserve_quote_amount,
        start_lp,
        end_base: pool.base_amount,
        end_quote: pool.quote_amount,
        end_lp: ctx.accounts.lp_mint.supply,
    });

    Ok(())
}

//
