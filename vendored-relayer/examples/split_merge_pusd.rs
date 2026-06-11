//! V2: split pUSD into YES + NO outcome tokens, then merge them back to pUSD.
//!
//! This is the post-V2-migration (2026-04-28) flow. Collateral is **pUSD**
//! (Polymarket USD, `0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB`), not USDC.e.
//! The call routes through the `CtfCollateralAdapter` (or the
//! `NegRiskCtfCollateralAdapter` for neg-risk markets), which:
//!
//!   split:  pull pUSD → unwrap to USDC.e → CTF.splitPosition → mint YES + NO
//!   merge:  pull YES + NO → CTF.mergePositions → wrap USDC.e → return pUSD
//!
//! Smart-contract wallets (Safe) are supported via the existing
//! `Safe.execTransaction` flow — V2 verifies the resulting Safe call through
//! Safe's native EIP-1271 `isValidSignature` (see `polymarket_relayer::eip1271`).
//!
//! ## Prerequisites
//!
//!   * Safe (or Proxy) is deployed for your EOA.
//!   * Wallet holds ≥ AMOUNT pUSD.
//!   * Approvals are in place (this example sets them up if APPROVE=1).
//!
//! ## Env
//!
//!   PRIVATE_KEY=0x...                EOA private key
//!   POLY_RELAYER_API_KEY=...         (or BUILDER_KEY + BUILDER_SECRET + BUILDER_PASSPHRASE)
//!   POLY_RELAYER_ADDRESS=0x...       Your EOA address (required by relayer-key auth)
//!   CONDITION_ID=0x...               32-byte condition id (binary market)
//!   AMOUNT=1000000                   pUSD amount in raw 6-decimal units (default 1 pUSD)
//!   SIGNATURE_TYPE=2                 1 = Proxy, 2 = Safe (default 2)
//!   MARKET_TYPE=regular              "regular" (default) or "neg_risk"
//!   APPROVE=0                        1 to first run setup_approvals_v2 in a separate tx
//!   STEP=both                        "both" (default), "split", or "merge"
//!   POLYGON_RPC_URL=https://...      (recommended: on-chain Safe nonce read)
//!
//! ## Run
//!
//!   cargo run --example split_merge_pusd

use ethers::signers::LocalWallet;
use ethers::types::U256;
use polymarket_relayer::{
    operations, AuthMethod, RelayClient, RelayerError, RelayerTxType,
};
use std::env;

fn parse_bytes32(hex_str: &str) -> anyhow::Result<[u8; 32]> {
    let stripped = hex_str.trim().strip_prefix("0x").unwrap_or(hex_str.trim());
    let bytes = hex::decode(stripped)
        .map_err(|e| anyhow::anyhow!("invalid hex for condition_id: {e}"))?;
    if bytes.len() != 32 {
        anyhow::bail!(
            "condition_id must be 32 bytes (got {}). Make sure to include the full hex.",
            bytes.len()
        );
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}

fn pick_auth() -> anyhow::Result<AuthMethod> {
    // Prefer Builder credentials (HMAC) if they're all present.
    let key = env::var("BUILDER_KEY").ok();
    let secret = env::var("BUILDER_SECRET").ok();
    let passphrase = env::var("BUILDER_PASSPHRASE").ok();
    if let (Some(k), Some(s), Some(p)) = (key.as_deref(), secret.as_deref(), passphrase.as_deref())
    {
        if !k.is_empty() && !s.is_empty() && !p.is_empty() {
            return Ok(AuthMethod::builder(k, s, p));
        }
    }

    let api_key = env::var("POLY_RELAYER_API_KEY")
        .map_err(|_| anyhow::anyhow!("set BUILDER_* env vars OR POLY_RELAYER_API_KEY"))?;
    let address = env::var("POLY_RELAYER_ADDRESS")
        .map_err(|_| anyhow::anyhow!("POLY_RELAYER_ADDRESS required when using relayer-key auth"))?;
    Ok(AuthMethod::relayer_key(&api_key, &address))
}

fn pick_tx_type() -> anyhow::Result<RelayerTxType> {
    let raw = env::var("SIGNATURE_TYPE").unwrap_or_else(|_| "2".to_string());
    let n: u8 = raw
        .parse()
        .map_err(|_| anyhow::anyhow!("SIGNATURE_TYPE must be 0, 1, or 2 (got '{raw}')"))?;
    RelayerTxType::from_signature_type(n)
        .ok_or_else(|| anyhow::anyhow!("SIGNATURE_TYPE must be 0, 1, or 2 (got {n})"))
}

#[derive(Debug, Clone, Copy)]
enum MarketKind {
    Regular,
    NegRisk,
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
        .map_err(|_| anyhow::anyhow!("CONDITION_ID env var required (0x-prefixed 32-byte hex)"))?;
    let amount_str = env::var("AMOUNT").unwrap_or_else(|_| "1000000".to_string());
    let do_approve = matches!(env::var("APPROVE").as_deref(), Ok("1") | Ok("true"));

    let wallet: LocalWallet = private_key.parse()?;
    let auth = pick_auth()?;
    let tx_type = pick_tx_type()?;
    let market = pick_market()?;
    let step = pick_step()?;

    let mut client = RelayClient::new(137, wallet, auth, tx_type).await?;
    if let Ok(rpc) = env::var("POLYGON_RPC_URL") {
        client.set_rpc_url(rpc);
    }

    let condition_id = parse_bytes32(&condition_id_hex)?;
    let amount = U256::from_dec_str(amount_str.trim())?;
    let partition: Vec<u64> = vec![1, 2]; // binary outcome (YES / NO)

    println!("─── V2 pUSD split / merge ───");
    println!("EOA:        {:?}", client.signer_address());
    println!("Wallet:     {:?} ({:?})", client.wallet_address()?, tx_type);
    println!("Market:     {:?}", market);
    println!("Condition:  0x{}", hex::encode(condition_id));
    println!("Amount:     {} (raw 6-dec pUSD units)", amount);
    println!("Step:       {:?}", step);
    println!();

    // ── 0. Optional: refresh V2 approvals ─────────────────────────────
    if do_approve {
        println!("Setting up V2 approvals (pUSD + CTF for all V2 contracts)...");
        match client.setup_approvals_v2().await {
            Ok(handle) => {
                let result = handle.wait().await?;
                println!(
                    "✅ Approvals confirmed! tx: {}",
                    result.tx_hash.unwrap_or_default()
                );
            }
            Err(RelayerError::TransactionFailed(msg))
            | Err(RelayerError::TransactionInvalid(msg)) => {
                // approve() is idempotent; if it fails because allowance is
                // already set, surface but don't abort.
                eprintln!("⚠️  setup_approvals_v2 returned: {msg}");
                eprintln!("    Continuing — allowance may already be in place.");
            }
            Err(e) => return Err(e.into()),
        }
        println!();
    }

    // ── 1. Split: pUSD → YES + NO ─────────────────────────────────────
    if matches!(step, Step::Both | Step::Split) {
        let split_tx = match market {
            MarketKind::Regular => operations::split_pusd(condition_id, &partition, amount),
            MarketKind::NegRisk => {
                operations::split_pusd_neg_risk(condition_id, &partition, amount)
            }
        };
        println!(
            "Splitting {} pUSD via adapter → {:?} ...",
            amount, split_tx.to
        );
        let handle = client.execute(vec![split_tx], "Split pUSD (V2)").await?;
        let result = handle.wait().await?;
        println!(
            "✅ Split complete! tx: {}",
            result.tx_hash.unwrap_or_default()
        );
        println!();
    }

    // ── 2. Merge: YES + NO → pUSD ─────────────────────────────────────
    if matches!(step, Step::Both | Step::Merge) {
        let merge_tx = match market {
            MarketKind::Regular => operations::merge_pusd(condition_id, &partition, amount),
            MarketKind::NegRisk => {
                operations::merge_pusd_neg_risk(condition_id, &partition, amount)
            }
        };
        println!(
            "Merging {} of each outcome via adapter → {:?} ...",
            amount, merge_tx.to
        );
        let handle = client.execute(vec![merge_tx], "Merge pUSD (V2)").await?;
        let result = handle.wait().await?;
        println!(
            "✅ Merge complete! tx: {}",
            result.tx_hash.unwrap_or_default()
        );
    }

    Ok(())
}
