# rs-builder-relayer-client

Rust SDK for [Polymarket's gasless relayer](https://docs.polymarket.com/trading/gasless) — covering both the legacy Safe / Proxy flow and the **V2 Deposit Wallet (POLY_1271)** flow that replaced USDC.e with pUSD on 2026-04-28.

Redeem positions, approve tokens, split / merge — zero gas.

## What's in this SDK

| Flow | Wallet model | Collateral | Relayer `type` | API |
|---|---|---|---|---|
| **V1 (legacy)** | Gnosis Safe (`signature_type=2`) | USDC.e | `SAFE` / `SAFE-CREATE` | `client.execute(...)`, `client.deploy()` |
| **V1 (legacy)** | Polymarket Proxy (`signature_type=1`, e.g. magic.link) | USDC.e | `PROXY` | `client.execute(...)` |
| **V2** | Deposit Wallet (UUPS ERC-1967 proxy, EIP-1271) | **pUSD** | `WALLET` / `WALLET-CREATE` | `client.execute_deposit_wallet_batch(...)`, `client.deploy_deposit_wallet()` |
| **Fallback** | any | — | direct on-chain | `DirectExecutor` (pays MATIC gas) |

V2 is the canonical flow for new users; V1 still works for already-deployed Safes/Proxies.

## 30-Second Quickstart

```bash
cargo new my-redeemer && cd my-redeemer
cargo add rs-builder-relayer-client ethers tokio --features tokio/full
cargo add anyhow dotenvy hex
```

Create `.env` (see [`.env.example`](.env.example) for the full list):
```
PRIVATE_KEY=0x...
BUILDER_KEY=...
BUILDER_SECRET=...
BUILDER_PASSPHRASE=...
# Optional. polygon-rpc.com is unstable — use Alchemy/QuickNode.
POLYGON_RPC_URL=https://...
```

`src/main.rs` — set up a legacy Safe wallet and approve USDC.e:
```rust
use polymarket_relayer::{RelayClient, AuthMethod, RelayerTxType};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let wallet = std::env::var("PRIVATE_KEY")?.parse()?;
    let mut client = RelayClient::new(
        137, wallet,
        AuthMethod::builder(
            &std::env::var("BUILDER_KEY")?,
            &std::env::var("BUILDER_SECRET")?,
            &std::env::var("BUILDER_PASSPHRASE")?,
        ),
        RelayerTxType::Safe,
    ).await?;

    if let Ok(rpc) = std::env::var("POLYGON_RPC_URL") {
        client.set_rpc_url(rpc);   // on-chain Safe nonce — avoids GS026
    }

    client.setup_approvals().await?.wait().await?;
    println!("Done.");
    Ok(())
}
```

```bash
cargo run
```

## Getting Your Credentials

| Credential | Where |
|---|---|
| `PRIVATE_KEY` | Your Polygon wallet private key (MetaMask > Account Details > Export) |
| Relayer API key | [polymarket.com/settings > Relayer API Keys](https://polymarket.com/settings) (anyone) |

No Builder keys? Use `AuthMethod::relayer_key("key", "address")` — same features, simpler setup.

---

## Install

```toml
[dependencies]
rs-builder-relayer-client = "0.1"
ethers = "2"
tokio = { version = "1", features = ["full"] }
anyhow = "1"
dotenvy = "0.15"
hex = "0.4"
```

---

## V1: Redeem (Safe / Proxy)

Add `CONDITION_ID=0x...` to your `.env`, then:

```rust
use polymarket_relayer::{AuthMethod, RelayClient, RelayerTxType, operations};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let wallet = std::env::var("PRIVATE_KEY")?.parse()?;
    let client = RelayClient::new(
        137,
        wallet,
        AuthMethod::builder(
            &std::env::var("BUILDER_KEY")?,
            &std::env::var("BUILDER_SECRET")?,
            &std::env::var("BUILDER_PASSPHRASE")?,
        ),
        RelayerTxType::Safe,
    ).await?;

    let condition_id_hex = std::env::var("CONDITION_ID")?;
    let condition_id_bytes = hex::decode(condition_id_hex.trim_start_matches("0x"))?;
    let mut cid = [0u8; 32];
    cid.copy_from_slice(&condition_id_bytes);

    let tx = operations::redeem_regular(cid, &[1, 2]);
    let result = client.execute(vec![tx], "Redeem").await?.wait().await?;
    println!("Transaction Hash: {:?}", result.tx_hash);

    Ok(())
}
```

---

## V2: Deposit Wallet — split / merge with pUSD

The Deposit Wallet is a deterministic UUPS ERC-1967 proxy deployed by the Polymarket factory. It holds your pUSD, validates batches via EIP-712 / EIP-1271, and is the `msg.sender` to the `CtfCollateralAdapter`.

### 1. Derive the wallet address (no RPC needed)

```rust
let address = client.derive_deposit_wallet_address()?;
println!("Deposit Wallet: {:?}", address);
```

Matches the TypeScript SDK's `await relayer.deriveDepositWalletAddress()` byte-for-byte (Solady `LibClone.initCodeHashERC1967` derivation).

### 2. Deploy it (once per EOA)

```rust
if !client.is_deposit_wallet_deployed().await? {
    let result = client.deploy_deposit_wallet().await?;
    println!("Deployed at tx {}", result.tx_hash.unwrap_or_default());
}
```

### 3. Fund it

Transfer pUSD (`0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB`) from your EOA to the derived Deposit Wallet address. The relayer **cannot** do this for you — it's a one-time MetaMask send.

### 4. Execute a batch (approve + split + merge in one tx)

```rust
use polymarket_relayer::{operations, contracts, DepositWalletCall};
use ethers::types::U256;
use std::time::{SystemTime, UNIX_EPOCH};

let condition_id_bytes = hex::decode(std::env::var("CONDITION_ID")?.trim_start_matches("0x"))?;
let mut cid = [0u8; 32];
cid.copy_from_slice(&condition_id_bytes);
let amount = U256::from(2_000_000u64); // 2 pUSD (6 decimals)

// All Transaction helpers convert into a DepositWalletCall via .into()
let calls: Vec<DepositWalletCall> = vec![
    operations::approve_pusd_for_ctf_adapter().into(),
    operations::approve_ctf_for_ctf_adapter().into(),
    operations::split_pusd(cid, &[1, 2], amount).into(),
    operations::merge_pusd(cid, &[1, 2], amount).into(),
];

let deadline = SystemTime::now()
    .duration_since(UNIX_EPOCH)?
    .as_secs() + 240; // 4 minutes from now

let result = client
    .execute_deposit_wallet_batch(calls, None, deadline, Some("split+merge demo"))
    .await?
    .wait()
    .await?;
println!("Batch tx: {:?}", result.tx_hash);
```

`None` for the second arg = derive the wallet address from the signer.

> **Why EIP-1271?** The V2 CTF Exchange verifies orders signed by smart-contract wallets through `isValidSignature(bytes32,bytes)`. The Deposit Wallet implements this natively; split/merge themselves don't need a 1271 signature (they go through the wallet's EIP-712 `executeBatch`), but trading on V2 does. See [`src/eip1271.rs`](src/eip1271.rs) for the signing helpers.

### How V2 Split & Merge Work (pUSD)

In V2, Polymarket transitioned from `USDC.e` to `pUSD` (a wrapper around USDC.e) as the primary collateral. Because the core `ConditionalTokens` framework (CTF) contract is immutable and expects the original collateral (USDC.e), Polymarket introduced the **CtfCollateralAdapter** (`0xAdA100...`).

When you use the Deposit Wallet to execute `split` or `merge` operations with pUSD:

1. **Split (Minting YES/NO)**: The Deposit Wallet calls `splitPosition` on the **Adapter**. The Adapter takes your `pUSD`, unwraps it into `USDC.e`, splits the `USDC.e` into `YES` and `NO` condition tokens via the core CTF contract, and sends those condition tokens back to your Deposit Wallet.
2. **Merge (Redeeming pUSD)**: The Deposit Wallet calls `mergePositions` on the **Adapter**, providing equal amounts of `YES` and `NO` tokens. The Adapter merges them back into `USDC.e` via the CTF, wraps the `USDC.e` back into `pUSD`, and returns the `pUSD` to your Deposit Wallet.

Since these operations require moving tokens, you must first include standard `approve` (for pUSD) and `setApprovalForAll` (for CTF condition tokens) instructions in your Deposit Wallet batch, targeted at the Adapter.


---

## API

### Operations (build a single `Transaction`)

| Operation | V1 (USDC.e) | V2 (pUSD via adapter) |
|---|---|---|
| Redeem regular | `operations::redeem_regular(cid, &[1, 2])` | (same — redeem returns pUSD via adapter automatically) |
| Redeem neg-risk | `operations::redeem_neg_risk_positions(cid, &[1, 2])` | (same) |
| Split | `operations::split_regular(cid, &[1, 2], amount)` | `operations::split_pusd(cid, &[1, 2], amount)` |
| Split (neg-risk) | — | `operations::split_pusd_neg_risk(cid, &[1, 2], amount)` |
| Merge | `operations::merge_regular(cid, &[1, 2], amount)` | `operations::merge_pusd(cid, &[1, 2], amount)` |
| Merge (neg-risk) | — | `operations::merge_pusd_neg_risk(cid, &[1, 2], amount)` |
| ERC20 approve | `operations::approve(token, spender, U256::MAX)` | same |
| ERC1155 approve | `operations::set_approval_for_all(ctf, op, true)` | same |
| Setup V1 approvals | `operations::approve_usdc_for_*`, `approve_ctf_for_*` | — |
| Setup V2 approvals | — | `operations::approve_pusd_for_ctf_adapter` etc. + `client.setup_approvals_v2()` |

### Client methods

| Method | V1 | V2 |
|---|---|---|
| Deploy wallet | `client.deploy()` (Safe only) | `client.deploy_deposit_wallet()` |
| Check deployed | `client.is_deployed()` | `client.is_deposit_wallet_deployed()` |
| Derive address | `client.wallet_address()` | `client.derive_deposit_wallet_address()` |
| Get nonce | `client.get_nonce()` (Safe / Proxy) | `client.get_deposit_wallet_nonce()` |
| Single / batch | `client.execute(vec![tx], "desc")` | `client.execute_deposit_wallet_batch(calls, None, deadline, Some("desc"))` |
| Multi-send batch | `client.execute_batch(vec![tx1, tx2], "desc")` | (use a single deposit wallet batch instead) |
| Sequential chunks | `client.execute_sequential(vec![vec![tx1], vec![tx2]], None, None)` | (still works for V1) |
| Direct on-chain fallback | `DirectExecutor::new(rpc_url, wallet, 137)?` | — |

---

## Auth

```rust
// Builder API keys (HMAC — recommended; works with V1 and V2)
AuthMethod::builder("key", "secret", "passphrase")

// Relayer API keys (from polymarket.com/settings > API Keys)
AuthMethod::relayer_key("api_key", "wallet_address")
```

---

## Direct Fallback (when relayer returns 429)

> **Warning:** Do **not** use `https://polygon-rpc.com/` as your RPC URL — it frequently causes TLS handshake EOF errors and connection resets, especially under load. Use a dedicated provider:
> - [Alchemy](https://www.alchemy.com/) (recommended): `https://polygon-mainnet.g.alchemy.com/v2/YOUR_KEY`
> - [QuickNode](https://www.quicknode.com/): `https://YOUR_ENDPOINT.quiknode.pro/YOUR_KEY/`
> - [LlamaRPC](https://llamarpc.com/): `https://polygon.llamarpc.com`

```rust
use polymarket_relayer::{DirectExecutor, RelayerError};

let rpc_url = std::env::var("POLYGON_RPC_URL")
    .expect("Set POLYGON_RPC_URL to an Alchemy/QuickNode endpoint");

// Safe wallet (signature_type=2, default)
let direct = DirectExecutor::new(&rpc_url, wallet, 137)?;

// Proxy wallet (signature_type=1, e.g. magic.link)
let direct = DirectExecutor::new_proxy(&rpc_url, wallet, 137)?;

// Proxy with explicit address (when derived address differs)
let direct = DirectExecutor::new_proxy_with_address(&rpc_url, wallet, 137, proxy_addr)?;

match client.execute(vec![tx], "Redeem").await {
    Err(RelayerError::QuotaExhausted) => {
        let result = direct.execute(&tx).await?;  // pays gas in MATIC
    }
    other => { /* handle normally */ }
}
```

Direct fallback currently covers V1 (Safe / Proxy). V2 Deposit Wallet `execute()` calldata can be built and sent via any signer; raise an issue if you need a built-in helper.

---

## Batching & Execution Strategies

The right strategy depends on which wallet flow you're using.

### V2 Deposit Wallet
* **`client.execute_deposit_wallet_batch`** — preferred. All calls share one EIP-712 signature, one relayer round-trip, one on-chain tx. No nonce-collision risk. Use this for everything in V2.

### V1 Safe
* **`client.execute` / `client.execute_batch`** — Safe uses the official Gnosis `MultiSend` contract; multiple operations are packed tightly into a single tx. Heavy batching (> 2 ops) is fine here.

### V1 Proxy
* **`client.execute`** with ≤ 2 calls — the Polymarket relayer bot imposes per-tx gas limits at the top level; > 2 ops with Proxy often hits silent `relay hub: internal transaction failure` errors. The SDK dynamically scales request gas up to a hard cap of 400K.
* **`client.execute_sequential`** — for many proxy operations (e.g. redeeming 10 positions). Submits batches step-by-step, waiting for `STATE_CONFIRMED` between them to prevent nonce collisions across Gelato's relay pools.

---

## Contracts

All addresses on Polygon mainnet (chain 137). Constants live in [`src/contracts.rs`](src/contracts.rs).

### V1 (legacy)
| Contract | Address |
|---|---|
| USDC.e | `0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174` |
| Conditional Tokens (CTF) | `0x4D97DCd97eC945f40cF65F87097ACe5EA0476045` |
| CTF Exchange | `0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E` |
| Neg Risk CTF Exchange | `0xC5d563A36AE78145C45a50134d48A1215220f80a` |
| Neg Risk Adapter | `0xd91E80cF2E7be2e162c6513ceD06f1dD0dA35296` |
| Safe Factory | `0xaacFeEa03eb1561C4e67d661e40682Bd20E3541b` |
| Proxy Factory | `0xaB45c5A4B0c941a2F231C04C3f49182e1A254052` |

### V2 (post-2026-04-28 migration)
| Contract | Address |
|---|---|
| **pUSD** (collateral) | `0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB` |
| CTF Exchange V2 | `0xE111180000d2663C0091e4f400237545B87B996B` |
| Neg Risk CTF Exchange V2 | `0xe2222d279d744050d28e00520010520000310F59` |
| **CtfCollateralAdapter** | `0xAdA100Db00Ca00073811820692005400218FcE1f` |
| **NegRiskCtfCollateralAdapter** | `0xadA2005600Dec949baf300f4C6120000bDB6eAab` |
| **Deposit Wallet Factory** | `0x00000000000Fb5C9ADea0298D729A0CB3823Cc07` |
| Deposit Wallet Implementation | `0x58CA52ebe0DadfdF531Cde7062e76746de4Db1eB` |
| CollateralOnramp (USDC → pUSD) | `0x93070a847efEf7F70739046A929D47a521F5B8ee` |
| CollateralOfframp (pUSD → USDC) | `0x2957922Eb93258b93368531d39fAcCA3B4dC5854` |

---

## Examples

```bash
cp .env.example .env   # fill in your keys

# ── V1 (legacy USDC.e flow) ──
cargo run --example setup_wallet                # deploy Safe + V1 approvals
cargo run --example redeem_all                  # dry-run: scan positions
cargo run --example redeem_all -- --execute     # actually redeem
cargo run --example redeem_single               # redeem one position
cargo run --example split_merge                 # USDC.e split/merge demo
cargo run --example redeem_magic                # magic.link proxy wallet redeem

# ── V2 (pUSD via collateral adapter, Safe / Proxy wallet) ──
cargo run --example split_merge_pusd            # pUSD split/merge via CtfCollateralAdapter

# ── V2 Deposit Wallet (POLY_1271) ──
DEPLOY=1 cargo run --example split_merge_pusd_deposit_wallet              # deploy + run
APPROVE=1 STEP=both cargo run --example split_merge_pusd_deposit_wallet   # approve + split + merge
STEP=split cargo run --example split_merge_pusd_deposit_wallet            # split only
STEP=merge cargo run --example split_merge_pusd_deposit_wallet            # merge only
```

---

## EIP-1271 helpers

For signing V2 orders with a smart-contract wallet (Safe or Deposit Wallet), the [`eip1271`](src/eip1271.rs) module ships:

* `EIP1271_MAGIC_VALUE = 0x1626ba7e` — the `isValidSignature(bytes32,bytes)` magic return
* `sign_hash_for_safe(wallet, hash)` — raw 32-byte ECDSA sign in `r||s||v` form (no eth-sign prefix)
* `is_valid_signature_calldata(hash, signature)` — encode the verification call for an `eth_call`
* `is_magic_value(response)` — check the call returned the magic value
* `safe_prevalidated_signature(owner)` — Safe's `v=1` pre-approved-hash signature

These are **not** needed for the relayer flows in this SDK; they exist as glue for projects layering an orderbook client on top.

---

## References

- [Polymarket V2 Migration Guide](https://docs.polymarket.com/v2-migration)
- [Deposit Wallets (V2)](https://docs.polymarket.com/trading/deposit-wallets)
- [Gasless Docs](https://docs.polymarket.com/trading/gasless)
- [Polymarket Python SDK](https://github.com/Polymarket/py-builder-relayer-client)
- [Polymarket TypeScript SDK](https://github.com/Polymarket/builder-relayer-client)
- [CTF Exchange V2 source](https://github.com/Polymarket/ctf-exchange-v2)

## Donate

**Ethereum / Polygon:** `0xF4c6635dFfB53f21c500c1604EC284f8A8a7150D`
