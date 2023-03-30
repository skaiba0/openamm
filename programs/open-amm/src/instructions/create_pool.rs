use crate::errors::OpenAmmErrorCode;
use crate::stableswap::calculate_stableswap_lp_minted;
use crate::state::*;
use crate::util::{get_orderbook, init, pool_authority_seeds};
use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{mint_to, transfer, Mint, MintTo, Token, TokenAccount, Transfer};
use safe_transmute::to_bytes::transmute_to_bytes;
use serum_dex::state::{Market, OpenOrders};
use std::convert::identity;

use anchor_spl::dex;

use std::mem::size_of;
pub const LP_MINT_SEED: &str = "pool-lp-mint";
pub const MINIMUM_LIQUIDITY: u16 = 1000;

const QUOTE_VAULT_SEED: &str = "pool-quote-vault";
const BASE_VAULT_SEED: &str = "pool-base-vault";
const OPEN_ORDERS_SEED: &str = "pool-open-orders";
pub const POOL_SEED: &str = "pool";

const OPENBOOK_PADDING: usize = 12;

#[derive(Accounts)]
#[instruction(pool_type: u8)]
pub struct CreatePool<'info> {
    #[account(
        init,
        seeds = [pool.key().as_ref(), QUOTE_VAULT_SEED.as_bytes().as_ref()],
        bump,
        payer = signer,
        token::mint = quote_mint,
        token::authority = pool,
    )]
    pub quote_vault: Box<Account<'info, TokenAccount>>,

    #[account(
        init,
        seeds = [pool.key().as_ref(), BASE_VAULT_SEED.as_bytes().as_ref()],
        bump,
        payer = signer,
        token::mint = base_mint,
        token::authority = pool,
    )]
    pub base_vault: Box<Account<'info, TokenAccount>>,
    #[account(
        mut,
        token::mint = base_mint,
        token::authority = signer.key()
    )]
    pub signer_base: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = quote_mint,
        token::authority = signer.key()
    )]
    pub signer_quote: Box<Account<'info, TokenAccount>>,

    #[account(
        init,
        mint::decimals = 6,
        mint::authority = pool,
        seeds = [pool.key().as_ref(), LP_MINT_SEED.as_bytes().as_ref()],
        bump,
        payer = signer,
    )]
    pub lp_mint: Box<Account<'info, Mint>>,

    #[account(
        init,
        associated_token::mint = lp_mint,
        associated_token::authority = signer,
        payer = signer,
    )]
    pub signer_lp: Box<Account<'info, TokenAccount>>,

    #[account(
        constraint = market_accounts.open_orders.key() == open_orders.key()
            @ OpenAmmErrorCode::WrongOpenOrdersAccount,
    )]
    pub market_accounts: MarketAccounts<'info>,

    pub quote_mint: Box<Account<'info, Mint>>,

    pub base_mint: Box<Account<'info, Mint>>,
    #[account(
        init,
        seeds = [
            market_accounts.market.key().as_ref(),
            pool_type.to_le_bytes().as_ref(),
            POOL_SEED.as_bytes().as_ref()
        ],
        bump,
        payer = signer,
        space = size_of::<OpenAmmPool>() + 8,
        constraint = quote_mint.key() != base_mint.key() @ OpenAmmErrorCode::InvalidPair,
    )]
    pub pool: AccountLoader<'info, OpenAmmPool>,
    #[account(mut)]
    pub signer: Signer<'info>,

    /// CHECK
    #[account(
        init,
        seeds = [pool.key().as_ref(), OPEN_ORDERS_SEED.as_bytes().as_ref()],
        bump,
        payer = signer,
        owner = dex::ID,
        space = size_of::<OpenOrders>() + OPENBOOK_PADDING
    )]
    pub open_orders: AccountInfo<'info>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,

    #[account(address = dex::ID)]
    pub dex_program: Program<'info, dex::Dex>,
    pub rent: Sysvar<'info, Rent>,
}

pub fn handler<'info>(
    ctx: Context<'_, '_, '_, 'info, CreatePool<'info>>,
    pool_type: PoolType,
    initial_base_amount: u64,
    initial_quote_amount: u64,
) -> Result<()> {
    let cpi_token_program = ctx.accounts.token_program.to_account_info();
    let pool_bump = ctx.bumps.get("pool").unwrap().clone();
    let market_key = ctx.accounts.market_accounts.market.key();
    let pool_type_bytes = (pool_type as u8).to_le_bytes();
    let seeds = pool_authority_seeds!(
        market_key = market_key,
        pool_type_bytes = pool_type_bytes,
        bump = pool_bump
    );
    let pool_signer = &[&seeds[..]];

    let market = &ctx.accounts.market_accounts.market;
    let market_state = Market::load(&market, &dex::ID, false).unwrap();
    require!(
        ctx.accounts.base_mint.key().as_ref()
            == transmute_to_bytes(&identity(market_state.coin_mint)),
        OpenAmmErrorCode::MarketBaseMintMismatch,
    );
    require!(
        ctx.accounts.quote_mint.key().as_ref()
            == transmute_to_bytes(&identity(market_state.pc_mint)),
        OpenAmmErrorCode::MarketQuoteMintMismatch,
    );
    drop(market_state);

    let mut pool = ctx.accounts.pool.load_init()?;

    init! {
        pool = OpenAmmPool {
            market: ctx.accounts.market_accounts.market.clone().key(),
            quote_vault: ctx.accounts.quote_vault.key(),
            base_vault: ctx.accounts.base_vault.key(),
            cumulative_base_volume: 0,
            cumulative_quote_volume: 0,
            refund_base_amount: 0,
            refund_quote_amount: 0,
            open_orders: ctx.accounts.open_orders.key(),
            lp_mint: ctx.accounts.lp_mint.clone().key(),
            pool_type: pool_type,
            client_order_id: 1,
            bump: pool_bump,
            mm_active: true,
            base_mint: ctx.accounts.base_mint.key(),
            quote_mint: ctx.accounts.quote_mint.key(),
            base_decimals: ctx.accounts.base_mint.decimals,
            quote_decimals: ctx.accounts.quote_mint.decimals,
            base_amount: initial_base_amount,
            quote_amount: initial_quote_amount,
            placed_asks: [PlacedOrder::default(); 10],
            placed_bids: [PlacedOrder::default(); 10],
        }
    }
    drop(pool);

    let init_open_orders_cpi_ctx = CpiContext::new_with_signer(
        ctx.accounts.dex_program.to_account_info(),
        dex::InitOpenOrders {
            open_orders: ctx.accounts.open_orders.clone(),
            authority: ctx.accounts.pool.to_account_info(),
            market: ctx
                .accounts
                .market_accounts
                .market
                .clone()
                .to_account_info(),
            rent: ctx.accounts.rent.to_account_info(),
        },
        pool_signer,
    );
    dex::init_open_orders(init_open_orders_cpi_ctx)?;

    let transfer_base_to_pool_cpi_ctx = CpiContext::new(
        cpi_token_program.clone(),
        Transfer {
            from: ctx.accounts.signer_base.to_account_info(),
            to: ctx.accounts.base_vault.to_account_info(),
            authority: ctx.accounts.signer.to_account_info(),
        },
    );
    transfer(transfer_base_to_pool_cpi_ctx, initial_base_amount)?;

    let transfer_quote_to_pool_cpi_ctx = CpiContext::new(
        cpi_token_program.clone(),
        Transfer {
            from: ctx.accounts.signer_quote.to_account_info(),
            to: ctx.accounts.quote_vault.to_account_info(),
            authority: ctx.accounts.signer.to_account_info(),
        },
    );
    transfer(transfer_quote_to_pool_cpi_ctx, initial_quote_amount)?;

    let orderbook = get_orderbook(
        1,
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

    orderbook.place_new_orders(&ctx.accounts.base_vault, &ctx.accounts.quote_vault)?;

    let lp_minted: u64 = match pool_type {
        PoolType::XYK => ((initial_base_amount as u128)
            .checked_mul(initial_quote_amount as u128)
            .unwrap()
            .checked_sub(MINIMUM_LIQUIDITY.into())
            .unwrap() as f64)
            .sqrt() as u64,
        PoolType::STABLE => calculate_stableswap_lp_minted(
            0,
            0,
            0,
            initial_base_amount,
            initial_quote_amount,
            ctx.accounts.base_mint.decimals,
            ctx.accounts.quote_mint.decimals,
        ),
    };

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

    Ok(())
}
