import { BN } from '@project-serum/anchor';
import * as anchor from '@project-serum/anchor';
import {
  SystemProgram,
  PublicKey,
  Keypair,
  Transaction,
  Connection,
} from '@solana/web3.js';

import {
  TOKEN_PROGRAM_ID,
  mintTo,
  getOrCreateAssociatedTokenAccount,
  Account as TokenAccount,
} from '@solana/spl-token';
import {
  TokenInstructions,
  DexInstructions,
  Market,
} from '@project-serum/serum';

export const DEX_PID = new PublicKey(
  'srmqPvymJeFKQ4zGQed1GFppgkRHL9kaELCbyksJtPX',
);

export async function setupStableMarketWithOrders(
  provider: anchor.Provider,
  payer: Keypair,
  baseMint: PublicKey,
  quoteMint: PublicKey,
) {
  const asks = [
    [1.003, 3],
    [1.004, 4],
    [1.005, 5],
    [1.006, 6],
  ];
  const bids = [
    [0.997, 3],
    [0.996, 4],
    [0.995, 5],
    [0.994, 6],
  ];

  const marketCreator = await fundAccount(provider, payer, [
    baseMint,
    quoteMint,
  ]);

  const MARKET_A_USDC = await setupMarket(
    provider,
    marketCreator,
    payer,
    baseMint,
    quoteMint,
    bids,
    asks,
  );

  return MARKET_A_USDC;
}

export async function setupTestMarketWithOrders(
  provider: anchor.Provider,
  payer: Keypair,
  baseMint: PublicKey,
  quoteMint: PublicKey,
) {
  const asks = [
    [6.041, 7.8],
    [6.051, 72.3],
    [6.055, 5.4],
    [6.067, 15.7],
    [6.077, 390.0],
    [6.09, 24.0],
    [6.11, 36.3],
    [6.133, 300.0],
    [6.167, 687.8],
  ];
  const bids = [
    [6.004, 8.5],
    [5.995, 12.9],
    [5.987, 6.2],
    [5.978, 15.3],
    [5.965, 82.8],
    [5.961, 25.4],
  ];

  const marketCreator = await fundAccount(provider, payer, [
    baseMint,
    quoteMint,
  ]);

  const MARKET_A_USDC = await setupMarket(
    provider,
    marketCreator,
    payer,
    baseMint,
    quoteMint,
    bids,
    asks,
  );

  return MARKET_A_USDC;
}

export async function setupTestMarket(
  provider: anchor.Provider,
  payer: Keypair,
  baseMint: PublicKey,
  quoteMint: PublicKey,
) {
  const asks = [];
  const bids = [];

  const marketCreator = await fundAccount(provider, payer, [
    baseMint,
    quoteMint,
  ]);

  const MARKET_A_USDC = await setupMarket(
    provider,
    marketCreator,
    payer,
    baseMint,
    quoteMint,
    bids,
    asks,
  );

  return MARKET_A_USDC;
}

async function listMarket(
  provider: anchor.Provider,
  payer: Keypair,
  baseMint: PublicKey,
  quoteMint: PublicKey,
  baseLotSize: number,
  quoteLotSize: number,
) {
  const connection = provider.connection;

  const market = new Keypair();
  const requestQueue = new Keypair();
  const eventQueue = new Keypair();
  const bids = new Keypair();
  const asks = new Keypair();
  const baseVault = new Keypair();
  const quoteVault = new Keypair();
  const quoteDustThreshold = new BN(100);
  const feeRateBps = 0;

  const [vaultOwner, vaultSignerNonce] = await getVaultOwnerAndNonce(
    market.publicKey,
    DEX_PID,
  );

  const tx1 = new Transaction();
  tx1.add(
    SystemProgram.createAccount({
      fromPubkey: payer.publicKey,
      newAccountPubkey: baseVault.publicKey,
      lamports: await connection.getMinimumBalanceForRentExemption(165),
      space: 165,
      programId: TOKEN_PROGRAM_ID,
    }),
    SystemProgram.createAccount({
      fromPubkey: payer.publicKey,
      newAccountPubkey: quoteVault.publicKey,
      lamports: await connection.getMinimumBalanceForRentExemption(165),
      space: 165,
      programId: TOKEN_PROGRAM_ID,
    }),
    TokenInstructions.initializeAccount({
      account: baseVault.publicKey,
      mint: baseMint,
      owner: vaultOwner,
    }),
    TokenInstructions.initializeAccount({
      account: quoteVault.publicKey,
      mint: quoteMint,
      owner: vaultOwner,
    }),
  );

  const tx2 = new Transaction();
  tx2.add(
    SystemProgram.createAccount({
      fromPubkey: payer.publicKey,
      newAccountPubkey: market.publicKey,
      lamports: await connection.getMinimumBalanceForRentExemption(
        Market.getLayout(DEX_PID).span,
      ),
      space: Market.getLayout(DEX_PID).span,
      programId: DEX_PID,
    }),
    SystemProgram.createAccount({
      fromPubkey: payer.publicKey,
      newAccountPubkey: requestQueue.publicKey,
      lamports: await connection.getMinimumBalanceForRentExemption(5120 + 12),
      space: 5120 + 12,
      programId: DEX_PID,
    }),
    SystemProgram.createAccount({
      fromPubkey: payer.publicKey,
      newAccountPubkey: eventQueue.publicKey,
      lamports: await connection.getMinimumBalanceForRentExemption(262144 + 12),
      space: 262144 + 12,
      programId: DEX_PID,
    }),
    SystemProgram.createAccount({
      fromPubkey: payer.publicKey,
      newAccountPubkey: bids.publicKey,
      lamports: await connection.getMinimumBalanceForRentExemption(65536 + 12),
      space: 65536 + 12,
      programId: DEX_PID,
    }),
    SystemProgram.createAccount({
      fromPubkey: payer.publicKey,
      newAccountPubkey: asks.publicKey,
      lamports: await connection.getMinimumBalanceForRentExemption(65536 + 12),
      space: 65536 + 12,
      programId: DEX_PID,
    }),
    DexInstructions.initializeMarket({
      market: market.publicKey,
      requestQueue: requestQueue.publicKey,
      eventQueue: eventQueue.publicKey,
      bids: bids.publicKey,
      asks: asks.publicKey,
      baseVault: baseVault.publicKey,
      quoteVault: quoteVault.publicKey,
      baseMint,
      quoteMint,
      baseLotSize: new BN(baseLotSize),
      quoteLotSize: new BN(quoteLotSize),
      feeRateBps,
      vaultSignerNonce,
      quoteDustThreshold,
      programId: DEX_PID,
    }),
  );

  await provider.sendAndConfirm(tx1, [baseVault, quoteVault]);
  await provider.sendAndConfirm(tx2, [
    market,
    requestQueue,
    eventQueue,
    bids,
    asks,
  ]);

  return market.publicKey;
}

async function setupMarket(
  provider: anchor.Provider,
  marketCreator: MarketCreator,
  payer: Keypair,
  baseMint: PublicKey,
  quoteMint: PublicKey,
  bids: number[][],
  asks: number[][],
) {
  const marketAPublicKey = await listMarket(
    provider,
    payer,
    baseMint,
    quoteMint,
    100000,
    100,
  );

  const MARKET_A_USDC = await Market.load(
    provider.connection,
    marketAPublicKey,
    { commitment: 'recent' },
    DEX_PID,
  );

  const asksPromises = asks.map(async ask => {
    const { transaction, signers } =
      await MARKET_A_USDC.makePlaceOrderTransaction(provider.connection, {
        owner: marketCreator.keypair.publicKey,
        payer: marketCreator.tokens[baseMint.toString()].address,
        side: 'sell',
        price: ask[0],
        size: ask[1],
        orderType: 'postOnly',
        clientId: undefined,
        openOrdersAddressKey: undefined,
        openOrdersAccount: undefined,
        feeDiscountPubkey: null,
        selfTradeBehavior: 'abortTransaction',
      });

    const keypairs = signers.map(account =>
      Keypair.fromSecretKey(account.secretKey),
    );
    keypairs.push(marketCreator.keypair);

    const res = await provider.sendAndConfirm(transaction, keypairs);
  });

  const bidPromises = bids.map(async bid => {
    const { transaction, signers } =
      await MARKET_A_USDC.makePlaceOrderTransaction(provider.connection, {
        owner: marketCreator.keypair.publicKey,
        payer: marketCreator.tokens[quoteMint.toString()].address,
        side: 'buy',
        price: bid[0],
        size: bid[1],
        orderType: 'postOnly',
        clientId: undefined,
        openOrdersAddressKey: undefined,
        openOrdersAccount: undefined,
        feeDiscountPubkey: null,
        selfTradeBehavior: 'abortTransaction',
      });

    const keypairs = signers.map(account =>
      Keypair.fromSecretKey(account.secretKey),
    );
    keypairs.push(marketCreator.keypair);

    await provider.sendAndConfirm(transaction, keypairs);
  });

  await Promise.all([...asksPromises, ...bidPromises]);
  return MARKET_A_USDC;
}

export async function getVaultOwnerAndNonce(
  market: PublicKey,
  dexProgramId = DEX_PID,
): Promise<[PublicKey, BN]> {
  const nonce = new BN(0);
  while (nonce.toNumber() < 255) {
    try {
      const vaultOwner = await PublicKey.createProgramAddress(
        [market.toBuffer(), nonce.toArrayLike(Buffer, 'le', 8)],
        dexProgramId,
      );
      return [vaultOwner, nonce];
    } catch (e) {
      nonce.iaddn(1);
    }
  }
  throw new Error('Unable to find nonce');
}

export async function getVaultOwner(market: PublicKey, dexProgramId = DEX_PID) {
  const vaultOwner = await PublicKey.findProgramAddressSync(
    [market.toBuffer()],
    dexProgramId,
  );
}

// @project-serum/common is outdated
export async function createMintAndVault(
  provider: anchor.Provider,
  amount: BN,
  owner: PublicKey,
  decimals?: number,
): Promise<[PublicKey, PublicKey]> {
  const mint = new Keypair();
  const vault = new Keypair();
  const tx = new Transaction();
  tx.add(
    SystemProgram.createAccount({
      fromPubkey: owner,
      newAccountPubkey: mint.publicKey,
      space: 82,
      lamports: await provider.connection.getMinimumBalanceForRentExemption(82),
      programId: TokenInstructions.TOKEN_PROGRAM_ID,
    }),
    TokenInstructions.initializeMint({
      mint: mint.publicKey,
      decimals: decimals ?? 0,
      mintAuthority: owner,
    }),
    SystemProgram.createAccount({
      fromPubkey: owner,
      newAccountPubkey: vault.publicKey,
      space: 165,
      lamports: await provider.connection.getMinimumBalanceForRentExemption(
        165,
      ),
      programId: TokenInstructions.TOKEN_PROGRAM_ID,
    }),
    TokenInstructions.initializeAccount({
      account: vault.publicKey,
      mint: mint.publicKey,
      owner,
    }),
    TokenInstructions.mintTo({
      mint: mint.publicKey,
      destination: vault.publicKey,
      amount,
      mintAuthority: owner,
    }),
  );
  await provider.sendAndConfirm(tx, [mint, vault]);
  return [mint.publicKey, vault.publicKey];
}

type MarketCreator = {
  keypair: Keypair;
  tokens: Record<string, TokenAccount>;
};

async function fundAccount(
  provider: anchor.Provider,
  payer: Keypair,
  mints: PublicKey[],
): Promise<MarketCreator> {
  const amount = 100000 * 10 ** 6;
  const MARKET_MAKER = new Keypair();

  // const marketMaker = {
  //   tokens: {},
  //   keypair: MARKET_MAKER,
  // };

  const tokens: Record<string, TokenAccount> = {};

  // Transfer lamports to market maker.

  const tx = new Transaction();
  tx.add(
    SystemProgram.transfer({
      fromPubkey: payer.publicKey,
      toPubkey: MARKET_MAKER.publicKey,
      lamports: 100000000000,
    }),
  );
  await provider.sendAndConfirm(tx);

  // Transfer SPL tokens to the market maker.
  mints.forEach(async mint => {
    const mintTokenAccount = await getOrCreateAssociatedTokenAccount(
      provider.connection,
      payer,
      mint,
      MARKET_MAKER.publicKey,
    );

    await mintTo(
      provider.connection,
      payer,
      mint,
      mintTokenAccount.address,
      payer,
      amount,
    );

    tokens[mint.toString()] = mintTokenAccount;
  });

  return {
    keypair: MARKET_MAKER,
    tokens,
  };
}

export async function getAllBids(market: Market, provider: anchor.Provider) {
  const loadedBids = [];
  const bids = (await market.loadBids(provider.connection)).items();
  while (true) {
    const bid = bids.next();
    if (bid.done) {
      break;
    }
    loadedBids.push(bid);
  }
  return loadedBids;
}

export async function getAllAsks(market: Market, provider: anchor.Provider) {
  const loadedAsks = [];
  const asks = (await market.loadAsks(provider.connection)).items();
  while (true) {
    const ask = asks.next();
    if (ask.done) {
      break;
    }
    loadedAsks.push(ask);
  }
  return loadedAsks;
}

export async function getAllOrders(market: Market, provider: anchor.Provider) {
  return Promise.all([
    getAllBids(market, provider),
    getAllAsks(market, provider),
  ]);
}

export type Address = PublicKey | string;
export async function fetchData(
  connection: Connection,
  address: Address,
): Promise<Buffer> {
  let data = (await connection.getAccountInfo(new PublicKey(address)))?.data;
  if (!data) {
    throw 'could not fetch account';
  }

  return data;
}

export async function printMarketOrders(
  market: Market,
  provider: anchor.Provider,
) {
  const bids = await getAllBids(market, provider);
  const asks = await getAllAsks(market, provider);

  console.log('bids');
  bids.forEach(bid => {
    console.log('price', bid.value.price, 'size', bid.value.size);
  });
  console.log('asks');
  asks.forEach(ask => {
    console.log('price', ask.value.price, 'size', ask.value.size);
  });
}

export async function getMarket(connection: Connection, address: Address) {
  const data = await fetchData(connection, address);
}
