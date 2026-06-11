//! V2 Deposit Wallet — EIP-712 batch signing + relayer request builder.
//!
//! A Deposit Wallet is a UUPS-upgradeable ERC-1967 proxy deployed by
//! [`crate::contracts::DEPOSIT_WALLET_FACTORY`] for each owner EOA. It exposes
//! `execute(Batch, signature)` which validates an EIP-712 batch signature and
//! dispatches each `Call` in order.
//!
//! ## Signed typed-data
//!
//! ```solidity
//! struct Call  { address target; uint256 value; bytes data; }
//! struct Batch { address wallet; uint256 nonce; uint256 deadline; Call[] calls; }
//! ```
//!
//! Domain: `{ name: "DepositWallet", version: "1", chainId, verifyingContract: wallet }`.
//!
//! Signature is `r(32) || s(32) || v(1)` with `v ∈ {27, 28}` (standard ECDSA).

use ethers::abi::{encode, Token};
use ethers::signers::LocalWallet;
use ethers::types::{Address, H256, U256};
use ethers::utils::keccak256;
use std::str::FromStr;

use crate::contracts;
use crate::error::{RelayerError, Result};
use crate::types::{DepositWalletCall, DepositWalletParams, TransactionRequest};

// ── EIP-712 type hashes ────────────────────────────────────────────

/// `keccak256("Call(address target,uint256 value,bytes data)")`
fn call_type_hash() -> H256 {
    H256::from(keccak256(b"Call(address target,uint256 value,bytes data)"))
}

/// `keccak256("Batch(address wallet,uint256 nonce,uint256 deadline,Call[] calls)Call(address target,uint256 value,bytes data)")`
///
/// The Batch type encoding includes the nested Call type, per EIP-712 §3.
fn batch_type_hash() -> H256 {
    H256::from(keccak256(
        b"Batch(address wallet,uint256 nonce,uint256 deadline,Call[] calls)Call(address target,uint256 value,bytes data)",
    ))
}

/// `keccak256("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)")`
fn domain_type_hash() -> H256 {
    H256::from(keccak256(
        b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)",
    ))
}

/// EIP-712 domain separator for a specific Deposit Wallet instance.
pub fn domain_separator(chain_id: u64, wallet_address: Address) -> H256 {
    let encoded = encode(&[
        Token::FixedBytes(domain_type_hash().as_bytes().to_vec()),
        Token::FixedBytes(
            keccak256(contracts::DEPOSIT_WALLET_DOMAIN_NAME.as_bytes()).to_vec(),
        ),
        Token::FixedBytes(
            keccak256(contracts::DEPOSIT_WALLET_DOMAIN_VERSION.as_bytes()).to_vec(),
        ),
        Token::Uint(U256::from(chain_id)),
        Token::Address(wallet_address),
    ]);
    H256::from(keccak256(encoded))
}

/// Hash a single `Call` struct per EIP-712.
fn hash_call(call: &DepositWalletCall) -> Result<H256> {
    let target = Address::from_str(&call.target)
        .map_err(|e| RelayerError::InvalidAddress(format!("call.target: {e}")))?;
    let value = U256::from_dec_str(&call.value)
        .map_err(|e| RelayerError::Abi(format!("call.value '{}': {e}", call.value)))?;
    let data_bytes = hex::decode(call.data.strip_prefix("0x").unwrap_or(&call.data))
        .map_err(|e| RelayerError::Abi(format!("call.data hex: {e}")))?;
    let data_hash = keccak256(&data_bytes);

    Ok(H256::from(keccak256(encode(&[
        Token::FixedBytes(call_type_hash().as_bytes().to_vec()),
        Token::Address(target),
        Token::Uint(value),
        Token::FixedBytes(data_hash.to_vec()),
    ]))))
}

/// Hash the `Call[]` array per EIP-712 (`keccak256(concat(call_hash_i ...))`).
fn hash_calls(calls: &[DepositWalletCall]) -> Result<H256> {
    let mut acc = Vec::with_capacity(calls.len() * 32);
    for call in calls {
        acc.extend_from_slice(hash_call(call)?.as_bytes());
    }
    Ok(H256::from(keccak256(&acc)))
}

/// Hash the full `Batch` struct.
pub fn batch_struct_hash(
    wallet: Address,
    nonce: U256,
    deadline: U256,
    calls: &[DepositWalletCall],
) -> Result<H256> {
    let calls_hash = hash_calls(calls)?;
    Ok(H256::from(keccak256(encode(&[
        Token::FixedBytes(batch_type_hash().as_bytes().to_vec()),
        Token::Address(wallet),
        Token::Uint(nonce),
        Token::Uint(deadline),
        Token::FixedBytes(calls_hash.as_bytes().to_vec()),
    ]))))
}

/// Final EIP-712 digest: `keccak256(0x1901 || domain || structHash)`.
pub fn eip712_digest(
    chain_id: u64,
    wallet: Address,
    nonce: U256,
    deadline: U256,
    calls: &[DepositWalletCall],
) -> Result<H256> {
    let domain = domain_separator(chain_id, wallet);
    let struct_hash = batch_struct_hash(wallet, nonce, deadline, calls)?;

    let mut buf = Vec::with_capacity(2 + 32 + 32);
    buf.extend_from_slice(&[0x19, 0x01]);
    buf.extend_from_slice(domain.as_bytes());
    buf.extend_from_slice(struct_hash.as_bytes());

    Ok(H256::from(keccak256(&buf)))
}

/// Sign a Deposit Wallet batch and return packed `r || s || v` (65 bytes).
///
/// Uses `LocalWallet::sign_hash` (raw 32-byte signing, no eth_sign prefix).
pub fn sign_batch(
    wallet: &LocalWallet,
    chain_id: u64,
    deposit_wallet: Address,
    nonce: U256,
    deadline: U256,
    calls: &[DepositWalletCall],
) -> Result<Vec<u8>> {
    let digest = eip712_digest(chain_id, deposit_wallet, nonce, deadline, calls)?;
    let sig = wallet
        .sign_hash(digest)
        .map_err(|e| RelayerError::Signing(e.to_string()))?;

    let mut bytes = Vec::with_capacity(65);
    let mut r = [0u8; 32];
    sig.r.to_big_endian(&mut r);
    bytes.extend_from_slice(&r);
    let mut s = [0u8; 32];
    sig.s.to_big_endian(&mut s);
    bytes.extend_from_slice(&s);
    let v = sig.v as u8;
    let v_final = if v < 27 { v + 27 } else { v };
    bytes.push(v_final);
    Ok(bytes)
}

// ── Relayer request builders ───────────────────────────────────────

/// Build a `type: "WALLET"` submit body that the relayer will dispatch
/// through the user's Deposit Wallet.
pub fn build_batch_request(
    wallet: &LocalWallet,
    chain_id: u64,
    owner: Address,
    deposit_wallet: Address,
    nonce: u64,
    deadline: u64,
    calls: Vec<DepositWalletCall>,
    metadata: Option<String>,
) -> Result<TransactionRequest> {
    let nonce_u256 = U256::from(nonce);
    let deadline_u256 = U256::from(deadline);

    let signature_bytes = sign_batch(
        wallet,
        chain_id,
        deposit_wallet,
        nonce_u256,
        deadline_u256,
        &calls,
    )?;

    Ok(TransactionRequest {
        tx_type: "WALLET".to_string(),
        from: format!("{:?}", owner),
        to: contracts::DEPOSIT_WALLET_FACTORY.to_string(),
        proxy_wallet: None,
        data: None,
        signature: Some(format!("0x{}", hex::encode(&signature_bytes))),
        nonce: Some(nonce.to_string()),
        signature_params: None,
        metadata,
        value: None,
        deposit_wallet_params: Some(DepositWalletParams {
            deposit_wallet: format!("{:?}", deposit_wallet),
            deadline: deadline.to_string(),
            calls,
        }),
    })
}

/// Build a `type: "WALLET-CREATE"` submit body to deploy a Deposit Wallet.
pub fn build_create_request(owner: Address) -> TransactionRequest {
    TransactionRequest {
        tx_type: "WALLET-CREATE".to_string(),
        from: format!("{:?}", owner),
        to: contracts::DEPOSIT_WALLET_FACTORY.to_string(),
        proxy_wallet: None,
        data: None,
        signature: None,
        nonce: None,
        signature_params: None,
        metadata: Some("Deploy Deposit Wallet".to_string()),
        value: None,
        deposit_wallet_params: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethers::signers::Signer;

    fn fixed_wallet() -> LocalWallet {
        "0x0000000000000000000000000000000000000000000000000000000000000001"
            .parse()
            .unwrap()
    }

    #[test]
    fn type_hashes_are_correct_eip712_strings() {
        // Sanity-check the exact strings being hashed.
        let h_call = keccak256(b"Call(address target,uint256 value,bytes data)");
        assert_eq!(call_type_hash().as_bytes(), h_call);
        let h_batch = keccak256(
            b"Batch(address wallet,uint256 nonce,uint256 deadline,Call[] calls)Call(address target,uint256 value,bytes data)",
        );
        assert_eq!(batch_type_hash().as_bytes(), h_batch);
    }

    #[test]
    fn empty_calls_array_hashes_to_keccak_of_empty() {
        let h = hash_calls(&[]).unwrap();
        // keccak256("") in keccak256() returns the empty-input hash.
        let expected = keccak256(b"");
        assert_eq!(h.as_bytes(), expected);
    }

    #[test]
    fn sign_batch_produces_65_bytes_with_v27_or_v28() {
        let wallet = fixed_wallet();
        let deposit_wallet: Address = "0x000000000000000000000000000000000000abcd"
            .parse()
            .unwrap();
        let calls = vec![DepositWalletCall::new(
            "0x0000000000000000000000000000000000000001",
            "0x12345678",
        )];
        let sig = sign_batch(
            &wallet,
            137,
            deposit_wallet,
            U256::zero(),
            U256::from(100u64),
            &calls,
        )
        .unwrap();
        assert_eq!(sig.len(), 65);
        assert!(matches!(sig[64], 27 | 28));
    }

    #[test]
    fn digest_changes_with_chain_id() {
        let deposit_wallet: Address = "0x000000000000000000000000000000000000abcd"
            .parse()
            .unwrap();
        let calls: Vec<DepositWalletCall> = vec![];
        let d1 = eip712_digest(137, deposit_wallet, U256::zero(), U256::zero(), &calls).unwrap();
        let d2 = eip712_digest(80002, deposit_wallet, U256::zero(), U256::zero(), &calls).unwrap();
        assert_ne!(d1, d2);
    }

    #[test]
    fn digest_changes_with_wallet_address() {
        let w1: Address = "0x0000000000000000000000000000000000000001"
            .parse()
            .unwrap();
        let w2: Address = "0x0000000000000000000000000000000000000002"
            .parse()
            .unwrap();
        let calls: Vec<DepositWalletCall> = vec![];
        let d1 = eip712_digest(137, w1, U256::zero(), U256::zero(), &calls).unwrap();
        let d2 = eip712_digest(137, w2, U256::zero(), U256::zero(), &calls).unwrap();
        assert_ne!(d1, d2);
    }

    #[test]
    fn build_create_request_has_minimal_fields() {
        let owner: Address = "0x000000000000000000000000000000000000abcd"
            .parse()
            .unwrap();
        let req = build_create_request(owner);
        assert_eq!(req.tx_type, "WALLET-CREATE");
        assert!(req.data.is_none());
        assert!(req.signature.is_none());
        assert!(req.deposit_wallet_params.is_none());
        // Serialised JSON should omit those optional keys entirely.
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("\"data\""));
        assert!(!json.contains("\"signature\""));
        assert!(!json.contains("depositWalletParams"));
    }

    #[test]
    fn build_batch_request_emits_camelcase_deposit_wallet_params() {
        let wallet = fixed_wallet();
        let owner = wallet.address();
        let deposit_wallet: Address = "0x000000000000000000000000000000000000abcd"
            .parse()
            .unwrap();
        let calls = vec![DepositWalletCall::new(
            "0x0000000000000000000000000000000000000001",
            "0x",
        )];
        let req = build_batch_request(
            &wallet,
            137,
            owner,
            deposit_wallet,
            0,
            100,
            calls,
            Some("test".to_string()),
        )
        .unwrap();
        let value: serde_json::Value = serde_json::to_value(&req).unwrap();
        // Top-level shape: matches the TS SDK's WALLET request body.
        assert_eq!(value["type"], "WALLET");
        assert_eq!(value["nonce"], "0");
        assert!(value["signature"].is_string());
        let params = &value["depositWalletParams"];
        assert!(params.is_object());
        assert_eq!(params["deadline"], "100");
        assert!(params["depositWallet"].is_string());
        assert!(params["calls"].is_array());
        // Optional top-level fields must be omitted entirely.
        let obj = value.as_object().unwrap();
        assert!(!obj.contains_key("data"));
        assert!(!obj.contains_key("signatureParams"));
        assert!(!obj.contains_key("proxyWallet"));
        assert!(!obj.contains_key("value"));
    }
}
