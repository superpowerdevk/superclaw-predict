use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use ethers::abi::{encode, Token};
use ethers::signers::Signer as _;
use ethers::types::{Address, U256};
use serde_json::Value;

use polymarket_relayer::builder::deposit_wallet::build_batch_request;
use polymarket_relayer::types::DepositWalletCall;
use polymarket_relayer::{AuthMethod, RelayClient, RelayerTxType, operations};

const NATIVE_USDC: &str = "0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359";
const USDCE: &str = "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174";
const ONRAMP: &str = "0x93070a847efEf7F70739046A929D47a521F5B8ee";
const ROUTER: &str = "0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45";
const NEG_RISK_ADAPTER: &str = "0xd91E80cF2E7be2e162c6513ceD06f1dD0dA35296";
const PUSD: &str = "0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB";

fn swap_native_to_usdce(amount_in: U256, min_out: U256, recipient: Address) -> DepositWalletCall {
    let sel = ethers::utils::id("exactInputSingle((address,address,uint24,address,uint256,uint256,uint160))");
    let params = Token::Tuple(vec![
        Token::Address(NATIVE_USDC.parse().unwrap()),
        Token::Address(USDCE.parse().unwrap()),
        Token::Uint(U256::from(100u64)),
        Token::Address(recipient),
        Token::Uint(amount_in),
        Token::Uint(min_out),
        Token::Uint(U256::zero()),
    ]);
    let mut data = sel.to_vec();
    data.extend_from_slice(&encode(&[params]));
    DepositWalletCall::new(ROUTER, format!("0x{}", hex::encode(data)))
}

fn wrap_usdce_to_pusd(amount: U256, to: Address) -> DepositWalletCall {
    let sel = ethers::utils::id("wrap(address,address,uint256)");
    let mut data = sel.to_vec();
    data.extend_from_slice(&encode(&[
        Token::Address(USDCE.parse().unwrap()),
        Token::Address(to),
        Token::Uint(amount),
    ]));
    DepositWalletCall::new(ONRAMP, format!("0x{}", hex::encode(data)))
}

fn approvals() -> Vec<DepositWalletCall> {
    vec![
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
    ]
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("approve");

    let home = std::env::var("HOME")?;
    let w: Value = serde_json::from_str(&fs::read_to_string(format!("{home}/.superclaw-predict/wallet.json"))?)?;
    let pk = w["private_key"].as_str().expect("private_key in wallet.json");
    let wallet: ethers::signers::LocalWallet = pk.parse()?;
    let owner = wallet.address();

    let mut client = RelayClient::new(137, wallet.clone(), AuthMethod::relayer_key("x", "x"), RelayerTxType::Safe).await?;
    if let Ok(rpc) = std::env::var("POLYGON_RPC_URL") { client.set_rpc_url(rpc); }

    let deposit = client.derive_deposit_wallet_address()?;
    let nonce = client.get_deposit_wallet_nonce().await?;
    let deadline = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() + 3600;

    let mut calls: Vec<DepositWalletCall> = Vec::new();

    if mode == "fund" {
        let micro = args.get(2).expect("usage: sign-batch fund <micro_native_usdc>");
        let amount = U256::from_dec_str(micro).expect("bad amount");
        let min_out = amount * U256::from(99u64) / U256::from(100u64);
        calls.push(operations::approve(NATIVE_USDC, ROUTER, amount).into());
        calls.push(swap_native_to_usdce(amount, min_out, deposit));
        calls.push(operations::approve(USDCE, ONRAMP, min_out).into());
        calls.push(wrap_usdce_to_pusd(min_out, deposit));
    } else if mode == "wrap" {
        let micro = args.get(2).expect("usage: sign-batch wrap <micro_usdce>");
        let amount = U256::from_dec_str(micro).expect("bad amount");
        calls.push(operations::approve(USDCE, ONRAMP, amount).into());
        calls.push(wrap_usdce_to_pusd(amount, deposit));
    }

    calls.extend(approvals());

    let req = build_batch_request(&wallet, 137, owner, deposit, nonce, deadline, calls, Some("DW setup".to_string()))?;
    println!("{}", serde_json::to_string(&req)?);
    Ok(())
}
