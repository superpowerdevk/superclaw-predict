use std::fs;
use serde_json::Value;
use polymarket_relayer::{RelayClient, AuthMethod, RelayerTxType, operations};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: redeem <condition_id_hex>");
        std::process::exit(2);
    }
    let cid_hex = args[1].trim_start_matches("0x");
    let cid_bytes = hex::decode(cid_hex)?;
    let mut cid = [0u8; 32];
    cid.copy_from_slice(&cid_bytes);

    let home = std::env::var("HOME")?;
    let w: Value =
        serde_json::from_str(&fs::read_to_string(format!("{home}/.superclaw-predict/wallet.json"))?)?;
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

    let tx = operations::redeem_regular(cid, &[1u64, 2u64]);
    let result = client.execute(vec![tx], "Redeem").await?.wait().await?;
    println!("REDEEM tx: {:?}", result.tx_hash);
    Ok(())
}
