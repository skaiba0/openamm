use anchor_lang::prelude::*;

#[error_code]
pub enum OpenAmmErrorCode {
    #[msg("OpenAmmErrorCode::InvalidPair - Pair is invalid")]
    InvalidPair,
    #[msg("OpenAmmErrorCode::WrongOpenOrdersAccount - Wrong open orders account for pool")]
    WrongOpenOrdersAccount,
    #[msg("OpenAmmErrorCode::WrongMarket - Wrong market account for pool")]
    WrongMarketAccount,
    #[msg("OpenAmmErrorCode::MarketBaseMintMismatch - Market base mint does not match token A")]
    MarketBaseMintMismatch,
    #[msg("OpenAmmErrorCode::MarketQuoteMintMismatch - Market quote mint does not match token B")]
    MarketQuoteMintMismatch,
    #[msg("OpenAmmErrorCode::SlippageBaseExceeded - Slippage for base exceeded")]
    SlippageBaseExceeded,
    #[msg("OpenAmmErrorCode::SlippageQuoteExceeded - Slippage for quote exceeded")]
    SlippageQuoteExceeded,
    #[msg("OpenAmmErrorCode::MarketMakingAlreadyActive - Market making is already active")]
    MarketMakingAlreadyActive,
    #[msg("OpenAmmErrorCode::OpenOrdersTokensLocked - Open orders tokens are locked")]
    OpenOrdersTokensLocked,
}
