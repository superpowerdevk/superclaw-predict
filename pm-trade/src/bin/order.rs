use std::fs;
use std::str::FromStr as _;
use std::time::{SystemTime, UNIX_EPOCH};

use alloy::primitives::Address;
use serde_json::Value;

use polymarket_client_sdk_v2::auth::{LocalSigner, Signer as _};
use polymarket_client_sdk_v2::clob::types::{Amount, Side, SignatureType};
use polymarket_client_sdk_v2::clob::{Client, Config};
use polymarket_client_sdk_v2::types::{Decimal, U256};
use polymarket_client_sdk_v2::POLYGON;

const HOST: &str = "https://clob.polymarket.com";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: order <buy|sell> <token_id> <amount>");
        std::process::exit(2);
    }
    let side_str = args[1].to_lowercase();
    let token_id = U256::from_str(&args[2])?;
    let amount: Decimal = args[3].parse()?;
    let (side, amt) = match side_str.as_str() {
        "buy" => (Side::Buy, Amount::usdc(amount)?),
        "sell" => (Side::Sell, Amount::shares(amount)?),
        _ => {
            eprintln!("side must be buy or sell");
            std::process::exit(2);
        }
    };

    let home = std::env::var("HOME")?;
    let w: Value =
        serde_json::from_str(&fs::read_to_string(format!("{home}/.superclaw-predict/wallet.json"))?)?;
    let pk = w["private_key"].as_str().expect("private_key in wallet.json");
    let deposit = Address::from_str(&std::env::var("DEPOSIT_WALLET")?)?;
    let signer = LocalSigner::from_str(pk)?.with_chain_id(Some(POLYGON));

    // Force the CREATE path (which funder-wraps the L1 auth correctly) instead of the buggy
    // DERIVE path: a fresh nonce each run => no existing key for it => create_or_derive creates.
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as u32;

    let client = Client::new(HOST, Config::builder().use_server_time(true).build())?
        .authentication_builder(&signer)
        .funder(deposit)
        .signature_type(SignatureType::Poly1271)
        .nonce(nonce)
        .authenticate()
        .await?;

    println!("authenticated (nonce {nonce}). {side_str} {amount} on token {token_id} ...");
    let order = client
        .market_order()
        .token_id(token_id)
        .amount(amt)
        .side(side)
        .build()
        .await?;
    let signed = client.sign(&signer, order).await?;
    let r = client.post_order(signed).await?;
    println!("ORDER PLACED: order_id={} success={}", r.order_id, r.success);
    Ok(())
}
