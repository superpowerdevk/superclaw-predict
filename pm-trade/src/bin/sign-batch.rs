use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use ethers::abi::{encode, Token};
use ethers::signers::Signer as _;
use ethers::types::{Address, U256};
use serde_json::Value;

use polymarket_relayer::builder::deposit_wallet::build_batch_request;
use polymarket_relayer::types::DepositWalletCall;
use polymarket_relayer::{AuthMethod, RelayClient, RelayerTxType, operations};

const USDCE: &str = "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174";
const ONRAMP: &str = "0x93070a847efEf7F70739046A929D47a521F5B8ee";
const NEG_RISK_ADAPTER: &str = "0xd91E80cF2E7be2e162c6513ceD06f1dD0dA35296";
const PUSD: &str = "0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB";

fn wrap_call(asset: &str, to: Address, amount: U256) -> DepositWalletCall {
    let sel = ethers::utils::id("wrap(address,address,uint256)");
    let mut data = sel.to_vec();
    let asset_addr: Address = asset.parse().expect("asset addr");
    data.extend_from_slice(&encode(&[
        Token::Address(asset_addr),
        Token::Address(to),
        Token::Uint(amount),
    ]));
    DepositWalletCall::new(ONRAMP, format!("0x{}", hex::encode(data)))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("approve");

    let home = std::env::var("HOME")?;
    let w: Value =
        serde_json::from_str(&fs::read_to_string(format!("{home}/.superclaw-predict/wallet.json"))?)?;
    let pk = w["private_key"].as_str().expect("private_key in wallet.json");
    let wallet: ethers::signers::LocalWallet = pk.parse()?;
    let owner = wallet.address();

    let mut client = RelayClient::new(137, wallet.clone(), AuthMethod::relayer_key("x", "x"), RelayerTxType::Safe).await?;
    if let Ok(rpc) = std::env::var("POLYGON_RPC_URL") { client.set_rpc_url(rpc); }

    let deposit_wallet = client.derive_deposit_wallet_address()?;
    let nonce = client.get_deposit_wallet_nonce().await?;
    let deadline = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() + 3600;

    let mut calls: Vec<DepositWalletCall> = Vec::new();

    if mode == "wrap" {
        let micro = args.get(2).expect("usage: sign-batch wrap <micro_usdc>");
        let amount = U256::from_dec_str(micro).expect("bad amount");
        calls.push(operations::approve(USDCE, ONRAMP, amount).into());
        calls.push(wrap_call(USDCE, deposit_wallet, amount));
    }

    calls.extend([
        operations::approve_pusd_for_ctf_exchange_v2().into(),
        operations::approve_pusd_for_neg_risk_exchange_v2().into(),
        operations::approve_pusd_for_ctf_adapter().into(),
        operations::approve_pusd_for_neg_risk_ctf_adapter().into(),
        operations::approve_ctf_for_ctf_exchange_v2().into(),
        operations::approve_ctf_for_neg_risk_exchange_v2().into(),
        operations::approve_ctf_for_ctf_adapter().into(),
        operations::approve_ctf_for_neg_risk_ctf_adapter().into(),
        operations::approve_ctf_for_neg_risk_adapter().into(),
        operations::approve(PUSD, NEG_RISK_ADAPTER, U256::MAX).into(),
    ]);

    let req = build_batch_request(&wallet, 137, owner, deposit_wallet, nonce, deadline, calls, Some("DW setup".to_string()))?;
    println!("{}", serde_json::to_string(&req)?);
    Ok(())
}
