/** 
//! V2 Deposit Wallet — split pUSD → YES + NO, then merge YES + NO → pUSD.
//!
//! Rust port of the user's working TypeScript example. The Deposit Wallet
//! (POLY_1271 smart-contract wallet) holds pUSD and is the `msg.sender`
//! that calls the `CtfCollateralAdapter`.
//!
//! Flow per relayer round-trip:
//!   1. Read WALLET nonce from `/nonce?type=WALLET`.
//!   2. EIP-712-sign `Batch{wallet, nonce, deadline, calls[]}`.
//!   3. POST `/submit` with `type: "WALLET"` + `depositWalletParams`.
//!
//! ## Prerequisites
//!
//!   * Deposit Wallet is deployed for your EOA (run with `DEPLOY=1` first).
//!   * The Deposit Wallet holds ≥ AMOUNT pUSD (you must transfer pUSD into the
//!     derived address before splitting).
//!   * Approvals are in place — `APPROVE=1` adds them to the same batch.
//!
//! ## Env
//!
//!   PRIVATE_KEY=0x...
//!   BUILDER_KEY / BUILDER_SECRET / BUILDER_PASSPHRASE   (HMAC, preferred)
//!     or POLY_RELAYER_API_KEY + POLY_RELAYER_ADDRESS    (relayer-key auth)
//!   CONDITION_ID=0x...           32-byte condition id (binary market)
//!   AMOUNT=2000000               raw 6-dec pUSD (default 2 pUSD)
//!   MARKET_TYPE=regular          "regular" | "neg_risk"
//!   STEP=both                    "both" | "split" | "merge"
//!   APPROVE=0                    1 to prepend approve calls into the batch
//!   DEPLOY=0                     1 to first deploy the Deposit Wallet
//!   DEADLINE_SECS=240            seconds from now until batch expires
//!   POLYGON_RPC_URL=https://...  (informational; not strictly required for WALLET flow)
//!
//! ## Run
//!
//!   cargo run --example split_merge_pusd_deposit_wallet
*/

use ethers::abi::{encode, Token};
use ethers::signers::{LocalWallet, Signer};
use ethers::types::{Address, U256};
use ethers::utils::keccak256;
use polymarket_relayer::{
    contracts, AuthMethod, DepositWalletCall, RelayClient, RelayerError, RelayerTxType,
};
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};

// ── Calldata builders (match the TS example) ──────────────────────

fn approve_calldata(spender: Address, amount: U256) -> String {
    let mut calldata = contracts::APPROVE_SELECTOR.to_vec();
    calldata.extend_from_slice(&encode(&[Token::Address(spender), Token::Uint(amount)]));
    format!("0x{}", hex::encode(&calldata))
}

fn set_approval_for_all_calldata(operator: Address, approved: bool) -> String {
    let mut calldata = contracts::SET_APPROVAL_FOR_ALL_SELECTOR.to_vec();
    calldata.extend_from_slice(&encode(&[Token::Address(operator), Token::Bool(approved)]));
    format!("0x{}", hex::encode(&calldata))
}

fn split_position_calldata(
    collateral: Address,
    condition_id: [u8; 32],
    partition: &[u64],
    amount: U256,
) -> String {
    let selector = keccak256(b"splitPosition(address,bytes32,bytes32,uint256[],uint256)");
    let mut calldata = selector[..4].to_vec();
    calldata.extend_from_slice(&encode(&[
        Token::Address(collateral),
        Token::FixedBytes(vec![0u8; 32]),
        Token::FixedBytes(condition_id.to_vec()),
        Token::Array(
            partition
                .iter()
                .map(|&i| Token::Uint(U256::from(i)))
                .collect(),
        ),
        Token::Uint(amount),
    ]));
    format!("0x{}", hex::encode(&calldata))
}

fn merge_positions_calldata(
    collateral: Address,
    condition_id: [u8; 32],
    partition: &[u64],
    amount: U256,
) -> String {
    let selector = keccak256(b"mergePositions(address,bytes32,bytes32,uint256[],uint256)");
    let mut calldata = selector[..4].to_vec();
    calldata.extend_from_slice(&encode(&[
        Token::Address(collateral),
        Token::FixedBytes(vec![0u8; 32]),
        Token::FixedBytes(condition_id.to_vec()),
        Token::Array(
            partition
                .iter()
                .map(|&i| Token::Uint(U256::from(i)))
                .collect(),
        ),
        Token::Uint(amount),
    ]));
    format!("0x{}", hex::encode(&calldata))
}

// ── Env / args ────────────────────────────────────────────────────

fn parse_bytes32(hex_str: &str) -> anyhow::Result<[u8; 32]> {
    let stripped = hex_str.trim().strip_prefix("0x").unwrap_or(hex_str.trim());
    let bytes = hex::decode(stripped)
        .map_err(|e| anyhow::anyhow!("invalid hex for condition_id: {e}"))?;
    if bytes.len() != 32 {
        anyhow::bail!(
            "condition_id must be 32 bytes (got {}). Include the full 0x-prefixed hex.",
            bytes.len()
        );
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}

fn pick_auth() -> anyhow::Result<AuthMethod> {
    if let (Ok(k), Ok(s), Ok(p)) = (
        env::var("BUILDER_KEY"),
        env::var("BUILDER_SECRET"),
        env::var("BUILDER_PASSPHRASE"),
    ) {
        if !k.is_empty() && !s.is_empty() && !p.is_empty() {
            return Ok(AuthMethod::builder(&k, &s, &p));
        }
    }
    let api_key = env::var("POLY_RELAYER_API_KEY")
        .map_err(|_| anyhow::anyhow!("set BUILDER_* OR POLY_RELAYER_API_KEY"))?;
    let addr = env::var("POLY_RELAYER_ADDRESS")
        .map_err(|_| anyhow::anyhow!("POLY_RELAYER_ADDRESS required for relayer-key auth"))?;
    Ok(AuthMethod::relayer_key(&api_key, &addr))
}

#[derive(Debug, Clone, Copy)]
enum MarketKind {
    Regular,
    NegRisk,
}

impl MarketKind {
    fn adapter(self) -> &'static str {
        match self {
            MarketKind::Regular => contracts::CTF_COLLATERAL_ADAPTER,
            MarketKind::NegRisk => contracts::NEG_RISK_CTF_COLLATERAL_ADAPTER,
        }
    }
}

fn pick_market() -> anyhow::Result<MarketKind> {
    match env::var("MARKET_TYPE")
        .unwrap_or_else(|_| "regular".to_string())
        .to_ascii_lowercase()
        .as_str()
    {
        "regular" | "binary" | "" => Ok(MarketKind::Regular),
        "neg_risk" | "negrisk" | "neg-risk" => Ok(MarketKind::NegRisk),
        other => anyhow::bail!("MARKET_TYPE must be 'regular' or 'neg_risk' (got '{other}')"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Both,
    Split,
    Merge,
}

fn pick_step() -> anyhow::Result<Step> {
    match env::var("STEP")
        .unwrap_or_else(|_| "both".to_string())
        .to_ascii_lowercase()
        .as_str()
    {
        "both" | "" => Ok(Step::Both),
        "split" => Ok(Step::Split),
        "merge" => Ok(Step::Merge),
        other => anyhow::bail!("STEP must be 'both', 'split', or 'merge' (got '{other}')"),
    }
}

fn env_flag(name: &str) -> bool {
    matches!(env::var(name).as_deref(), Ok("1") | Ok("true") | Ok("TRUE"))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ── Main ──────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init()
        .ok();

    let private_key = env::var("PRIVATE_KEY")
        .map_err(|_| anyhow::anyhow!("PRIVATE_KEY env var required"))?;
    let condition_id_hex = env::var("CONDITION_ID")
        .map_err(|_| anyhow::anyhow!("CONDITION_ID env var required"))?;
    let amount_str = env::var("AMOUNT").unwrap_or_else(|_| "2000000".to_string());
    let deadline_secs: u64 = env::var("DEADLINE_SECS")
        .unwrap_or_else(|_| "240".to_string())
        .parse()
        .map_err(|e| anyhow::anyhow!("DEADLINE_SECS must be integer: {e}"))?;

    let wallet: LocalWallet = private_key.parse()?;
    let auth = pick_auth()?;
    let market = pick_market()?;
    let step = pick_step()?;
    let do_approve = env_flag("APPROVE");
    let do_deploy = env_flag("DEPLOY");

    // We don't use Safe/Proxy flow here at all — the DepositWallet methods are
    // standalone. But the constructor requires a tx_type; pick Safe arbitrarily,
    // it is never used by the WALLET-typed calls.
    let client = RelayClient::new(137, wallet.clone(), auth, RelayerTxType::Safe).await?;

    let owner = wallet.address();
    let deposit_wallet_addr = client.derive_deposit_wallet_address()?;
    let condition_id = parse_bytes32(&condition_id_hex)?;
    let amount = U256::from_dec_str(amount_str.trim())?;
    let partition: Vec<u64> = vec![1, 2];
    let pusd: Address = contracts::PUSD.parse()?;
    let adapter: Address = market.adapter().parse()?;

    println!("─── V2 Deposit Wallet pUSD split/merge ───");
    println!("EOA owner:        {:?}", owner);
    println!("Deposit Wallet:   {:?}", deposit_wallet_addr);
    println!("Market:           {:?}", market);
    println!("Condition:        0x{}", hex::encode(condition_id));
    println!("Amount (raw):     {}", amount);
    println!("Step:             {:?}", step);
    println!("Adapter:          {:?}", adapter);
    println!();

   
    let mut calls: Vec<DepositWalletCall> = Vec::new();

 
    // `format!("{:?}", addr)` returns a full lower-case 0x-prefixed 40-char hex
    let adapter_str = market.adapter().to_string();

    if matches!(step, Step::Both | Step::Split) {
        calls.push(DepositWalletCall::new(
            adapter_str.clone(),
            split_position_calldata(pusd, condition_id, &partition, amount),
        ));
    }

    if matches!(step, Step::Both | Step::Merge) {
        calls.push(DepositWalletCall::new(
            adapter_str.clone(),
            merge_positions_calldata(pusd, condition_id, &partition, amount),
        ));
    }

    if calls.is_empty() {
        anyhow::bail!("Batch is empty — set APPROVE=1 or STEP=split|merge|both");
    }

    println!("Batch contents ({} call{}):", calls.len(), if calls.len() == 1 { "" } else { "s" });
    for (i, c) in calls.iter().enumerate() {
        let selector = c
            .data
            .strip_prefix("0x")
            .and_then(|d| d.get(0..8))
            .unwrap_or("????????");
        println!("  [{i}] -> {} (selector 0x{selector}, {} bytes data)",
            c.target,
            c.data.strip_prefix("0x").map(|d| d.len() / 2).unwrap_or(0),
        );
    }
    println!();

    let deadline = now_secs() + deadline_secs;
    println!("Deadline: {} ({} from now)", deadline, deadline_secs);

    let handle = match client
        .execute_deposit_wallet_batch(
            calls,
            Some(deposit_wallet_addr),
            deadline,
            Some("V2 pUSD split/merge via Deposit Wallet"),
        )
        .await
    {
        Ok(h) => h,
        Err(RelayerError::QuotaExhausted) => {
            anyhow::bail!("Relayer returned 429 (quota exhausted)");
        }
        Err(e) => return Err(e.into()),
    };
    println!("Tx ID: {}", handle.id());
    println!("Waiting for confirmation...");

    let result = handle.wait().await?;
    println!();
    println!("✅ Batch complete!");
    println!("  state:  {:?}", result.state);
    println!("  tx:     {}", result.tx_hash.unwrap_or_default());
    if let Some(err) = result.error {
        println!("  error: {err}");
    }

    Ok(())
}
