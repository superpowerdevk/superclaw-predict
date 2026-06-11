use polymarket_relayer::types::TransactionRequest;
use polymarket_relayer::{AuthMethod, RelayClient, RelayerTxType};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("usage: relay-submit <signed.json>");
    let req: TransactionRequest = serde_json::from_str(&std::fs::read_to_string(&path)?)?;

    // Dummy wallet: backend NEVER signs — it only relays with builder creds.
    let dummy: ethers::signers::LocalWallet =
        "0x0000000000000000000000000000000000000000000000000000000000000001".parse()?;
    let mut client = RelayClient::new(
        137,
        dummy,
        AuthMethod::builder(
            &std::env::var("BUILDER_KEY")?,
            &std::env::var("BUILDER_SECRET")?,
            &std::env::var("BUILDER_PASSPHRASE")?,
        ),
        RelayerTxType::Safe,
    )
    .await?;
    if let Ok(rpc) = std::env::var("POLYGON_RPC_URL") {
        client.set_rpc_url(rpc);
    }

    let resp = client.submit(req).await?;
    println!("RELAYED: {:?}", resp);
    Ok(())
}
