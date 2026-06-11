use anyhow::Result;
use serde_json::Value;
use std::fs;

use polymarket_relayer::{RelayClient, AuthMethod, RelayerTxType};

#[tokio::main]
async fn main() -> Result<()> {
    let home = std::env::var("HOME")?;
    let wpath = format!("{home}/.superclaw-predict/wallet.json");
    let w: Value = serde_json::from_str(&fs::read_to_string(&wpath)?)?;
    let pk = w["private_key"].as_str().expect("private_key in wallet.json");
    let wallet = pk.parse()?;

    let mut client = RelayClient::new(
        137,
        wallet,
        AuthMethod::builder(
            &std::env::var("BUILDER_KEY")?,
            &std::env::var("BUILDER_SECRET")?,
            &std::env::var("BUILDER_PASSPHRASE")?,
        ),
        RelayerTxType::Safe,
    ).await?;

    if let Ok(rpc) = std::env::var("POLYGON_RPC_URL") {
        client.set_rpc_url(rpc);
    }

    let deposit = client.derive_deposit_wallet_address()?;
    println!("DEPOSIT_WALLET = {deposit:?}");

    if !client.is_deposit_wallet_deployed().await? {
        println!("deploying deposit wallet (gasless)...");
        client.deploy_deposit_wallet().await?;
        println!("deployed.");
    } else {
        println!("deposit wallet already deployed.");
    }

    println!("setting up V2 approvals (gasless)...");
    client.setup_approvals_v2().await?.wait().await?;
    println!("DONE — deposit wallet ready. Fund it with pUSD next.");
    Ok(())
}
