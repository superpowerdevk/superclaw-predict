use std::fs;
use ethers::signers::Signer as _;
use serde_json::Value;
use polymarket_relayer::builder::deposit_wallet::build_batch_request;
use polymarket_relayer::types::DepositWalletCall;
use polymarket_relayer::{AuthMethod, RelayClient, RelayerTxType, operations};

const PUSD: &str = "0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let home = std::env::var("HOME")?;
    let w: Value =
        serde_json::from_str(&fs::read_to_string(format!("{home}/.superclaw-predict/wallet.json"))?)?;
    let pk = w["private_key"].as_str().expect("private_key in wallet.json");
    let wallet: ethers::signers::LocalWallet = pk.parse()?;
    let owner = wallet.address();

    // Dummy relayer auth: client only derives + reads nonce (both unauthenticated). Never submits.
    let mut client = RelayClient::new(
        137,
        wallet.clone(),
        AuthMethod::relayer_key("x", "x"),
        RelayerTxType::Safe,
    )
    .await?;
    if let Ok(rpc) = std::env::var("POLYGON_RPC_URL") {
        client.set_rpc_url(rpc);
    }

    let deposit_wallet = client.derive_deposit_wallet_address()?;
    let nonce = client.get_deposit_wallet_nonce().await?;
    let deadline = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        + 3600;

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
        operations::approve(PUSD, "0xd91E80cF2E7be2e162c6513ceD06f1dD0dA35296", ethers::types::U256::MAX).into(),
    ];

    let req = build_batch_request(&wallet, 137, owner, deposit_wallet, nonce, deadline, calls, Some("DW approvals".to_string()))?;
    // This signed JSON is the ONLY thing that leaves the client. No key, no creds in it beyond the signature.
    println!("{}", serde_json::to_string(&req)?);
    Ok(())
}
