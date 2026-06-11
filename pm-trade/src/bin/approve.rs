use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use ethers::types::U256;
use serde_json::Value;
use polymarket_relayer::types::DepositWalletCall;
use polymarket_relayer::{AuthMethod, RelayClient, RelayerTxType, operations};

const PUSD: &str = "0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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
    )
    .await?;
    if let Ok(rpc) = std::env::var("POLYGON_RPC_URL") {
        client.set_rpc_url(rpc);
    }

    let calls: Vec<DepositWalletCall> = vec![
        operations::approve_pusd_for_ctf_exchange_v2().into(),
        operations::approve_pusd_for_neg_risk_exchange_v2().into(),
        operations::approve_pusd_for_ctf_adapter().into(),
        operations::approve_pusd_for_neg_risk_ctf_adapter().into(),
        operations::approve_ctf_for_ctf_exchange_v2().into(),
        operations::approve_ctf_for_neg_risk_exchange_v2().into(),
        operations::approve_ctf_for_ctf_adapter().into(),
        operations::approve_ctf_for_neg_risk_ctf_adapter().into(),
        operations::approve_ctf_for_neg_risk_adapter().into(),
        // explicit: pUSD -> NegRiskAdapter (needed for neg-risk markets)
        operations::approve(PUSD, "0xd91E80cF2E7be2e162c6513ceD06f1dD0dA35296", U256::MAX).into(),
    ];

    let deadline = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() + 3600;

    let result = client
        .execute_deposit_wallet_batch(calls, None, deadline, Some("DW V2 approvals + negrisk"))
        .await?
        .wait()
        .await?;
    println!("APPROVALS SET (deposit wallet): {:?}", result.tx_hash);
    Ok(())
}
