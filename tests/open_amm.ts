import * as anchor from '@project-serum/anchor'
import { Program } from '@project-serum/anchor'
import { Market } from '@project-serum/serum'
import { OpenAmm } from '../target/types/open_amm'
import { PublicKey, Keypair, ComputeBudgetProgram } from '@solana/web3.js'
import {
  getVaultOwnerAndNonce,
  setupTestMarket,
  setupTestMarketWithOrders,
  setupStableMarketWithOrders,
  DEX_PID,
  getAllOrders,
} from './testUtils'
import { Account } from '@solana/spl-token'

import {
  mintTo,
  createMint,
  getOrCreateAssociatedTokenAccount,
  TOKEN_PROGRAM_ID,
  getAssociatedTokenAddress,
} from '@solana/spl-token'
import { assert } from 'chai'

const QUOTE_VAULT_SEED = 'pool-quote-vault'
const BASE_VAULT_SEED = 'pool-base-vault'
const OPEN_ORDERS_SEED = 'pool-open-orders'
const POOL_SEED = 'pool'

describe('openamm', () => {
  anchor.setProvider(anchor.AnchorProvider.env())

  const program = anchor.workspace.OpenAmm as Program<OpenAmm>
  const wallet = anchor.workspace.OpenAmm.provider.wallet.payer as Keypair
  let baseMint: PublicKey
  let quoteMint: PublicKey
  let baseMintWalletAta: Account
  let quoteMintWalletAta: Account
  let pool: PublicKey
  let signerLp: PublicKey
  let marketVaultSigner: PublicKey
  let lpMint: PublicKey
  let market: Market
  let openOrders: PublicKey
  let baseVault: PublicKey
  let quoteVault: PublicKey

  it('Sets up market and accounts', async () => {
    baseMint = await createMint(
      program.provider.connection,
      wallet,
      wallet.publicKey,
      wallet.publicKey,
      6
    )

    baseMintWalletAta = await getOrCreateAssociatedTokenAccount(
      program.provider.connection,
      wallet,
      baseMint,
      wallet.publicKey
    )

    await mintTo(
      program.provider.connection,
      wallet,
      baseMint,
      baseMintWalletAta.address,
      wallet,
      1000000000
    )

    quoteMint = await createMint(
      program.provider.connection,
      wallet,
      wallet.publicKey,
      wallet.publicKey,
      6
    )

    quoteMintWalletAta = await getOrCreateAssociatedTokenAccount(
      program.provider.connection,
      wallet,
      quoteMint,
      wallet.publicKey
    )

    await mintTo(
      program.provider.connection,
      wallet,
      quoteMint,
      quoteMintWalletAta.address,
      wallet,
      1000000000
    )

    market = await setupTestMarket(
      program.provider,
      wallet,
      baseMint,
      quoteMint
    )

    const [bids, asks] = await getAllOrders(market, program.provider)

    assert.strictEqual(bids.length, 0)
    assert.strictEqual(asks.length, 0)
  })

  it('Can create a pool with an empty market', async () => {
    pool = PublicKey.findProgramAddressSync(
      [
        market.publicKey.toBuffer(),
        new Uint8Array([0]),
        Buffer.from(POOL_SEED),
      ],
      program.programId
    )[0]

    openOrders = PublicKey.findProgramAddressSync(
      [pool.toBuffer(), Buffer.from('pool-open-orders')],
      program.programId
    )[0]

    lpMint = PublicKey.findProgramAddressSync(
      [pool.toBuffer(), Buffer.from('pool-lp-mint')],
      program.programId
    )[0]

    signerLp = await getAssociatedTokenAddress(lpMint, wallet.publicKey)

    marketVaultSigner = (await getVaultOwnerAndNonce(market.publicKey))[0]

    const additionalComputeBudgetInstruction =
      ComputeBudgetProgram.setComputeUnitLimit({ units: 800000 })

    const createPoolMethod = program.methods
      .createPool(
        /// CHECK: typescript error
        { xYK: {} },
        new anchor.BN('1000000000'),
        new anchor.BN('1000000000')
      )
      .accounts({
        baseMint,
        quoteMint,
        pool,
        signerBase: baseMintWalletAta.address,
        signerQuote: quoteMintWalletAta.address,
        lpMint,
        signerLp,
        openOrders,
        dexProgram: DEX_PID,
        tokenProgram: TOKEN_PROGRAM_ID,
        marketAccounts: {
          market: market.publicKey,
          requestQueue: market.decoded.requestQueue,
          eventQueue: market.decoded.eventQueue,
          bids: market.decoded.bids,
          asks: market.decoded.asks,
          baseVault: market.decoded.baseVault,
          quoteVault: market.decoded.quoteVault,
          vaultSigner: marketVaultSigner,
          openOrders,
        },
      })
      .preInstructions([additionalComputeBudgetInstruction])

    const pubkeys = await createPoolMethod.pubkeys()
    baseVault = pubkeys.baseVault
    quoteVault = pubkeys.quoteVault

    await createPoolMethod.rpc({ skipPreflight: true })

    const [bids, asks] = await getAllOrders(market, program.provider)
    assert.strictEqual(asks.length, 10)
    assert.strictEqual(bids.length, 9)

    const lpAmount = await program.provider.connection.getTokenAccountBalance(
      signerLp
    )
    assert.strictEqual(lpAmount.value.amount, '999999999')
    const poolAccount = await program.account.openAmmPool.fetch(pool)

    assert.strictEqual(poolAccount.baseAmount.toString(), '1000000000')
    assert.strictEqual(poolAccount.quoteAmount.toString(), '1000000000')
  })

  it('Can deposit to a pool', async () => {
    await Promise.all([
      mintTo(
        program.provider.connection,
        wallet,
        baseMint,
        baseMintWalletAta.address,
        wallet,
        1000000000
      ),
      mintTo(
        program.provider.connection,
        wallet,
        quoteMint,
        quoteMintWalletAta.address,
        wallet,
        1000000000
      ),
    ])
    const additionalComputeBudgetInstruction =
      ComputeBudgetProgram.setComputeUnitLimit({ units: 800000 })

    const depositMethod = program.methods
      .deposit(
        new anchor.BN('1000000000'),
        new anchor.BN('1000000000'),
        new anchor.BN('1000000000'),
        new anchor.BN('1000000000')
      )
      .accounts({
        pool,
        lpMint,
        signerLp,
        signerBase: baseMintWalletAta.address,
        signerQuote: quoteMintWalletAta.address,
        dexProgram: DEX_PID,
        marketAccounts: {
          market: market.publicKey,
          requestQueue: market.decoded.requestQueue,
          eventQueue: market.decoded.eventQueue,
          bids: market.decoded.bids,
          asks: market.decoded.asks,
          baseVault: market.decoded.baseVault,
          quoteVault: market.decoded.quoteVault,
          vaultSigner: marketVaultSigner,
          openOrders,
        },
      })
      .preInstructions([additionalComputeBudgetInstruction])

    await depositMethod.rpc()

    const lpAmount = await program.provider.connection.getTokenAccountBalance(
      signerLp
    )
    assert.strictEqual(lpAmount.value.amount, '1999999998')
    const poolAccount = await program.account.openAmmPool.fetch(pool)

    assert.strictEqual(poolAccount.baseAmount.toString(), '2000000000')
    assert.strictEqual(poolAccount.quoteAmount.toString(), '2000000000')
  })

  it('Can withdraw from a pool', async () => {
    const additionalComputeBudgetInstruction =
      ComputeBudgetProgram.setComputeUnitLimit({ units: 800000 })

    const withdrawMethod = program.methods
      .withdraw(new anchor.BN('999999999'))
      .accounts({
        pool,
        baseVault,
        quoteVault,
        lpMint,
        signerLp,
        signerBase: baseMintWalletAta.address,
        signerQuote: quoteMintWalletAta.address,
        dexProgram: DEX_PID,
        marketAccounts: {
          market: market.publicKey,
          requestQueue: market.decoded.requestQueue,
          eventQueue: market.decoded.eventQueue,
          bids: market.decoded.bids,
          asks: market.decoded.asks,
          baseVault: market.decoded.baseVault,
          quoteVault: market.decoded.quoteVault,
          vaultSigner: marketVaultSigner,
          openOrders,
        },
      })
      .preInstructions([additionalComputeBudgetInstruction])

    await withdrawMethod.rpc()

    const lpAmount = await program.provider.connection.getTokenAccountBalance(
      signerLp
    )
    assert.strictEqual(lpAmount.value.amount, '999999999')
    const poolAccount = await program.account.openAmmPool.fetch(pool)

    assert.strictEqual(poolAccount.baseAmount.toString(), '1000000000')
    assert.strictEqual(poolAccount.quoteAmount.toString(), '1000000000')
  })

  it('Can place an order that matches against program orders', async () => {
    let poolAccount = await program.account.openAmmPool.fetch(pool)
    const walletAsAccount = new anchor.web3.Account(wallet.secretKey)

    let quoteAmount = await program.provider.connection.getTokenAccountBalance(
      quoteMintWalletAta.address
    )
    let baseAmount = await program.provider.connection.getTokenAccountBalance(
      baseMintWalletAta.address
    )

    assert.strictEqual(quoteAmount.value.amount, '1000000000')
    assert.strictEqual(baseAmount.value.amount, '1000000000')

    assert.strictEqual(poolAccount.cumulativeQuoteVolume.toString(), '0')
    assert.strictEqual(poolAccount.cumulativeBaseVolume.toString(), '0')

    await market.placeOrder(program.provider.connection, {
      owner: walletAsAccount,
      payer: quoteMintWalletAta.address,
      side: 'buy', // 'buy' or 'sell'
      price: 10,
      size: 10,
      orderType: 'ioc', // 'limit', 'ioc', 'postOnly'
      feeDiscountPubkey: null,
    })

    const walletOpenOrders = await market.findOpenOrdersAccountsForOwner(
      program.provider.connection,
      wallet.publicKey
    )
    // Settle orders
    const promises = walletOpenOrders.map(async (openOrder) => {
      if (
        openOrder.baseTokenFree.toNumber() > 0 ||
        openOrder.quoteTokenFree.toNumber() > 0
      ) {
        await market.settleFunds(
          program.provider.connection,
          walletAsAccount,
          openOrder,
          baseMintWalletAta.address,
          quoteMintWalletAta.address
        )
      }
    })

    await Promise.all(promises)

    baseAmount = await program.provider.connection.getTokenAccountBalance(
      baseMintWalletAta.address
    )

    // Increased the amount bought
    assert.strictEqual(baseAmount.value.amount, '1010000000')
  })

  it('Can refresh orders', async () => {
    const additionalComputeBudgetInstruction =
      ComputeBudgetProgram.setComputeUnitLimit({ units: 800000 })

    const refreshMethod = program.methods
      .refreshOrders()
      .accounts({
        pool,
        marketAccounts: {
          market: market.publicKey,
          requestQueue: market.decoded.requestQueue,
          eventQueue: market.decoded.eventQueue,
          bids: market.decoded.bids,
          asks: market.decoded.asks,
          baseVault: market.decoded.baseVault,
          quoteVault: market.decoded.quoteVault,
          vaultSigner: marketVaultSigner,
          openOrders,
        },
        baseVault,
        quoteVault,
        signerBase: baseMintWalletAta.address,
        signerQuote: quoteMintWalletAta.address,
        dexProgram: DEX_PID,
      })
      .preInstructions([additionalComputeBudgetInstruction])

    await refreshMethod.rpc()

    const [bids, asks] = await getAllOrders(market, program.provider)

    assert.strictEqual(bids.length, 9)
    assert.strictEqual(asks.length, 10)
  })

  it('Can track cumulative volume correctly', async () => {
    const poolAccount = await program.account.openAmmPool.fetch(pool)
    const quoteAmount =
      await program.provider.connection.getTokenAccountBalance(
        quoteMintWalletAta.address
      )

    const quoteAmountChanged = new anchor.BN('1000000000').sub(
      new anchor.BN(quoteAmount.value.amount)
    )

    // Check: Will be slightly less than because of taker fees
    assert.ok(poolAccount.cumulativeQuoteVolume.lte(quoteAmountChanged))
    assert.strictEqual(poolAccount.cumulativeBaseVolume.toString(), '0')
  })

  it('Can create a test market with bids and asks', async () => {
    baseMint = await createMint(
      program.provider.connection,
      wallet,
      wallet.publicKey,
      wallet.publicKey,
      6
    )

    baseMintWalletAta = await getOrCreateAssociatedTokenAccount(
      program.provider.connection,
      wallet,
      baseMint,
      wallet.publicKey
    )

    await mintTo(
      program.provider.connection,
      wallet,
      baseMint,
      baseMintWalletAta.address,
      wallet,
      1000000000
    )

    quoteMint = await createMint(
      program.provider.connection,
      wallet,
      wallet.publicKey,
      wallet.publicKey,
      6
    )

    quoteMintWalletAta = await getOrCreateAssociatedTokenAccount(
      program.provider.connection,
      wallet,
      quoteMint,
      wallet.publicKey
    )

    await mintTo(
      program.provider.connection,
      wallet,
      quoteMint,
      quoteMintWalletAta.address,
      wallet,
      1000000000
    )

    market = await setupTestMarketWithOrders(
      program.provider,
      wallet,
      baseMint,
      quoteMint
    )

    const [bids, asks] = await getAllOrders(market, program.provider)

    assert.strictEqual(bids.length, 6)
    assert.strictEqual(asks.length, 9)
  })

  it('Can create a pool on a market that already has orders', async () => {
    pool = PublicKey.findProgramAddressSync(
      [
        market.publicKey.toBuffer(),
        new Uint8Array([0]),
        Buffer.from(POOL_SEED),
      ],
      program.programId
    )[0]

    openOrders = PublicKey.findProgramAddressSync(
      [pool.toBuffer(), Buffer.from('pool-open-orders')],
      program.programId
    )[0]

    lpMint = PublicKey.findProgramAddressSync(
      [pool.toBuffer(), Buffer.from('pool-lp-mint')],
      program.programId
    )[0]

    signerLp = await getAssociatedTokenAddress(lpMint, wallet.publicKey)

    marketVaultSigner = (await getVaultOwnerAndNonce(market.publicKey))[0]

    const additionalComputeBudgetInstruction =
      ComputeBudgetProgram.setComputeUnitLimit({ units: 800000 })

    const createPoolMethod = program.methods
      .createPool(
        { xYK: {} },
        new anchor.BN('1000000000'),
        new anchor.BN('1000000000')
      )
      .accounts({
        baseMint,
        quoteMint,
        pool,
        signerBase: baseMintWalletAta.address,
        signerQuote: quoteMintWalletAta.address,
        lpMint: lpMint,
        signerLp,
        openOrders,
        dexProgram: DEX_PID,
        tokenProgram: TOKEN_PROGRAM_ID,
        marketAccounts: {
          market: market.publicKey,
          requestQueue: market.decoded.requestQueue,
          eventQueue: market.decoded.eventQueue,
          bids: market.decoded.bids,
          asks: market.decoded.asks,
          baseVault: market.decoded.baseVault,
          quoteVault: market.decoded.quoteVault,
          vaultSigner: marketVaultSigner,
          openOrders,
        },
      })
      .preInstructions([additionalComputeBudgetInstruction])

    await createPoolMethod.rpc()

    const [bids, asks] = await getAllOrders(market, program.provider)

    assert.strictEqual(asks.length, 9)
    assert.strictEqual(bids.length, 15)

    const lpAmount = await program.provider.connection.getTokenAccountBalance(
      signerLp
    )
    assert.strictEqual(lpAmount.value.amount, '999999999')
    const poolAccount = await program.account.openAmmPool.fetch(pool)

    assert.strictEqual(poolAccount.baseAmount.toString(), '1000000000')
    assert.strictEqual(poolAccount.quoteAmount.toString(), '1000000000')
  })

  it('Can create a stable market', async () => {
    baseMint = await createMint(
      program.provider.connection,
      wallet,
      wallet.publicKey,
      wallet.publicKey,
      6
    )

    baseMintWalletAta = await getOrCreateAssociatedTokenAccount(
      program.provider.connection,
      wallet,
      baseMint,
      wallet.publicKey
    )

    await mintTo(
      program.provider.connection,
      wallet,
      baseMint,
      baseMintWalletAta.address,
      wallet,
      1000000000
    )

    quoteMint = await createMint(
      program.provider.connection,
      wallet,
      wallet.publicKey,
      wallet.publicKey,
      6
    )

    quoteMintWalletAta = await getOrCreateAssociatedTokenAccount(
      program.provider.connection,
      wallet,
      quoteMint,
      wallet.publicKey
    )

    await mintTo(
      program.provider.connection,
      wallet,
      quoteMint,
      quoteMintWalletAta.address,
      wallet,
      1000000000
    )

    market = await setupStableMarketWithOrders(
      program.provider,
      wallet,
      baseMint,
      quoteMint
    )

    const [bids, asks] = await getAllOrders(market, program.provider)

    assert.strictEqual(bids.length, 4)
    assert.strictEqual(asks.length, 4)
  })

  it('Can create a stable pool', async () => {
    pool = PublicKey.findProgramAddressSync(
      [
        market.publicKey.toBuffer(),
        new Uint8Array([1]),
        Buffer.from(POOL_SEED),
      ],
      program.programId
    )[0]

    openOrders = PublicKey.findProgramAddressSync(
      [pool.toBuffer(), Buffer.from('pool-open-orders')],
      program.programId
    )[0]

    lpMint = PublicKey.findProgramAddressSync(
      [pool.toBuffer(), Buffer.from('pool-lp-mint')],
      program.programId
    )[0]

    signerLp = await getAssociatedTokenAddress(lpMint, wallet.publicKey)

    marketVaultSigner = (await getVaultOwnerAndNonce(market.publicKey))[0]

    const additionalComputeBudgetInstruction =
      ComputeBudgetProgram.setComputeUnitLimit({ units: 900000 })

    const createPoolMethod = program.methods
      .createPool(
        { sTABLE: {} },
        new anchor.BN('1000000000'),
        new anchor.BN('1000000000')
      )
      .accounts({
        baseMint,
        quoteMint,
        pool,
        signerBase: baseMintWalletAta.address,
        signerQuote: quoteMintWalletAta.address,
        lpMint: lpMint,
        signerLp,
        openOrders,
        dexProgram: DEX_PID,
        tokenProgram: TOKEN_PROGRAM_ID,
        marketAccounts: {
          market: market.publicKey,
          requestQueue: market.decoded.requestQueue,
          eventQueue: market.decoded.eventQueue,
          bids: market.decoded.bids,
          asks: market.decoded.asks,
          baseVault: market.decoded.baseVault,
          quoteVault: market.decoded.quoteVault,
          vaultSigner: marketVaultSigner,
          openOrders,
        },
      })
      .preInstructions([additionalComputeBudgetInstruction])

    const pubkeys = await createPoolMethod.pubkeys()
    baseVault = pubkeys.baseVault
    quoteVault = pubkeys.quoteVault

    await createPoolMethod.rpc()

    const [bids, asks] = await getAllOrders(market, program.provider)

    assert.strictEqual(asks.length, 14)
    assert.strictEqual(bids.length, 13)

    const lpAmount = await program.provider.connection.getTokenAccountBalance(
      signerLp
    )
    assert.strictEqual(lpAmount.value.amount, '2000000000')
    const poolAccount = await program.account.openAmmPool.fetch(pool)

    assert.strictEqual(poolAccount.baseAmount.toString(), '1000000000')
    assert.strictEqual(poolAccount.quoteAmount.toString(), '1000000000')
  })

  it('Can replace orders after a market order is filled, refunding the cranker', async () => {
    const walletAsAccount = new anchor.web3.Account(wallet.secretKey)
    await mintTo(
      program.provider.connection,
      wallet,
      quoteMint,
      quoteMintWalletAta.address,
      wallet,
      100000000
    )
    let quoteBalance = await program.provider.connection.getTokenAccountBalance(
      quoteMintWalletAta.address
    )
    assert.strictEqual(quoteBalance.value.amount, '100000000')
    let [bids, asks] = await getAllOrders(market, program.provider)

    assert.strictEqual(bids.length, 13)
    assert.strictEqual(asks.length, 14)

    let poolAccount = await program.account.openAmmPool.fetch(pool)
    assert.strictEqual(poolAccount.refundBaseAmount.toString(), '0')
    assert.strictEqual(poolAccount.refundQuoteAmount.toString(), '0')

    await market.placeOrder(program.provider.connection, {
      owner: walletAsAccount,
      payer: quoteMintWalletAta.address,
      side: 'buy', // 'buy' or 'sell'
      price: 5,
      size: 10,
      orderType: 'ioc', // 'limit', 'ioc', 'postOnly'
      feeDiscountPubkey: null,
    })

    ;[bids, asks] = await getAllOrders(market, program.provider)
    poolAccount = await program.account.openAmmPool.fetch(pool)
    assert.strictEqual(poolAccount.refundBaseAmount.toString(), '0')
    assert.strictEqual(poolAccount.refundQuoteAmount.toString(), '0')

    assert.strictEqual(asks.length, 11)
    assert.strictEqual(bids.length, 13)

    const quoteAmountBefore =
      await program.provider.connection.getTokenAccountBalance(
        quoteMintWalletAta.address
      )

    const additionalComputeBudgetInstruction =
      ComputeBudgetProgram.setComputeUnitLimit({ units: 900000 })

    const refreshMethod = program.methods
      .refreshOrders()
      .accounts({
        pool,
        marketAccounts: {
          market: market.publicKey,
          requestQueue: market.decoded.requestQueue,
          eventQueue: market.decoded.eventQueue,
          bids: market.decoded.bids,
          asks: market.decoded.asks,
          baseVault: market.decoded.baseVault,
          quoteVault: market.decoded.quoteVault,
          vaultSigner: marketVaultSigner,
          openOrders,
        },
        baseVault,
        signerBase: baseMintWalletAta.address,
        signerQuote: quoteMintWalletAta.address,
        quoteVault,
        dexProgram: DEX_PID,
      })
      .preInstructions([additionalComputeBudgetInstruction])

    await refreshMethod.rpc()

    ;[bids, asks] = await getAllOrders(market, program.provider)
    assert.strictEqual(asks.length, 14)
    assert.strictEqual(bids.length, 13)

    const quoteAmountAfter =
      await program.provider.connection.getTokenAccountBalance(
        quoteMintWalletAta.address
      )

    // Should have gained some quote refund
    assert.ok(
      new anchor.BN(quoteAmountBefore.value.amount).lt(
        new anchor.BN(quoteAmountAfter.value.amount)
      )
    )
  })
})
