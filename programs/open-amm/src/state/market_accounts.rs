use anchor_lang::prelude::*;
use anchor_spl::token::TokenAccount;

#[derive(Accounts, Clone)]
pub struct MarketAccounts<'info> {
    /// CHECK:
    #[account(mut)]
    pub market: AccountInfo<'info>,
    /// CHECK:
    #[account(mut)]
    pub open_orders: AccountInfo<'info>,
    /// CHECK:
    #[account(mut)]
    pub request_queue: AccountInfo<'info>,
    /// CHECK:
    #[account(mut)]
    pub event_queue: AccountInfo<'info>,
    /// CHECK:
    #[account(mut)]
    pub bids: AccountInfo<'info>,
    /// CHECK:
    #[account(mut)]
    pub asks: AccountInfo<'info>,

    #[account(mut)]
    pub base_vault: Box<Account<'info, TokenAccount>>,

    #[account(mut)]
    pub quote_vault: Box<Account<'info, TokenAccount>>,

    /// CHECK:
    pub vault_signer: AccountInfo<'info>,
}
