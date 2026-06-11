use ethers::abi::encode;
use ethers::abi::Token;
use ethers::types::{Address, U256};
use ethers::utils::keccak256;

use crate::contracts;
use crate::types::Transaction;

/// Build a splitPosition(address,bytes32,bytes32,uint256[],uint256) transaction.
///
/// Splits collateral into outcome tokens.
pub fn split_position(
    collateral_token: &str,
    parent_collection: [u8; 32],
    condition_id: [u8; 32],
    partition: &[u64],
    amount: U256,
) -> Transaction {
    let collateral: Address = collateral_token.parse().expect("invalid collateral address");
    let selector = keccak256(b"splitPosition(address,bytes32,bytes32,uint256[],uint256)");

    let mut calldata = selector[..4].to_vec();
    calldata.extend_from_slice(&encode(&[
        Token::Address(collateral),
        Token::FixedBytes(parent_collection.to_vec()),
        Token::FixedBytes(condition_id.to_vec()),
        Token::Array(partition.iter().map(|&i| Token::Uint(U256::from(i))).collect()),
        Token::Uint(amount),
    ]));

    Transaction {
        to: contracts::CTF.to_string(),
        data: format!("0x{}", hex::encode(&calldata)),
        value: "0".to_string(),
    }
}

/// Build a mergePositions(address,bytes32,bytes32,uint256[],uint256) transaction.
///
/// Merges outcome tokens back into collateral.
pub fn merge_positions(
    collateral_token: &str,
    parent_collection: [u8; 32],
    condition_id: [u8; 32],
    partition: &[u64],
    amount: U256,
) -> Transaction {
    let collateral: Address = collateral_token.parse().expect("invalid collateral address");
    let selector = keccak256(b"mergePositions(address,bytes32,bytes32,uint256[],uint256)");

    let mut calldata = selector[..4].to_vec();
    calldata.extend_from_slice(&encode(&[
        Token::Address(collateral),
        Token::FixedBytes(parent_collection.to_vec()),
        Token::FixedBytes(condition_id.to_vec()),
        Token::Array(partition.iter().map(|&i| Token::Uint(U256::from(i))).collect()),
        Token::Uint(amount),
    ]));

    Transaction {
        to: contracts::CTF.to_string(),
        data: format!("0x{}", hex::encode(&calldata)),
        value: "0".to_string(),
    }
}

/// Convenience: split USDC.e with default parent collection (V1 / legacy path).
pub fn split_regular(condition_id: [u8; 32], partition: &[u64], amount: U256) -> Transaction {
    split_position(contracts::USDC_E, [0u8; 32], condition_id, partition, amount)
}

/// Convenience: merge back to USDC.e with default parent collection (V1 / legacy path).
pub fn merge_regular(condition_id: [u8; 32], partition: &[u64], amount: U256) -> Transaction {
    merge_positions(contracts::USDC_E, [0u8; 32], condition_id, partition, amount)
}

// ── V2: pUSD-aware split / merge via CtfCollateralAdapter ─────────────

/// Build a `splitPosition(...)` transaction targeted at a V2 collateral adapter.
///
/// The adapter has the **same function signature** as the underlying CTF, so
/// this just retargets the call from the CTF contract to the adapter contract.
/// The adapter pulls pUSD from `msg.sender`, unwraps to USDC.e under the hood,
/// calls `CTF.splitPosition`, and mints the YES/NO ERC-1155 outcome tokens back
/// to `msg.sender`.
///
/// `collateral_token` should be `contracts::PUSD` for V2 markets.
pub fn split_position_via_adapter(
    adapter: &str,
    collateral_token: &str,
    parent_collection: [u8; 32],
    condition_id: [u8; 32],
    partition: &[u64],
    amount: U256,
) -> Transaction {
    let collateral: Address = collateral_token.parse().expect("invalid collateral address");
    let selector = keccak256(b"splitPosition(address,bytes32,bytes32,uint256[],uint256)");

    let mut calldata = selector[..4].to_vec();
    calldata.extend_from_slice(&encode(&[
        Token::Address(collateral),
        Token::FixedBytes(parent_collection.to_vec()),
        Token::FixedBytes(condition_id.to_vec()),
        Token::Array(partition.iter().map(|&i| Token::Uint(U256::from(i))).collect()),
        Token::Uint(amount),
    ]));

    Transaction {
        to: adapter.to_string(),
        data: format!("0x{}", hex::encode(&calldata)),
        value: "0".to_string(),
    }
}

/// Build a `mergePositions(...)` transaction targeted at a V2 collateral adapter.
///
/// The adapter pulls YES + NO ERC-1155 tokens from `msg.sender`, calls
/// `CTF.mergePositions`, wraps the released USDC.e back into pUSD, and sends
/// the pUSD to `msg.sender`.
pub fn merge_positions_via_adapter(
    adapter: &str,
    collateral_token: &str,
    parent_collection: [u8; 32],
    condition_id: [u8; 32],
    partition: &[u64],
    amount: U256,
) -> Transaction {
    let collateral: Address = collateral_token.parse().expect("invalid collateral address");
    let selector = keccak256(b"mergePositions(address,bytes32,bytes32,uint256[],uint256)");

    let mut calldata = selector[..4].to_vec();
    calldata.extend_from_slice(&encode(&[
        Token::Address(collateral),
        Token::FixedBytes(parent_collection.to_vec()),
        Token::FixedBytes(condition_id.to_vec()),
        Token::Array(partition.iter().map(|&i| Token::Uint(U256::from(i))).collect()),
        Token::Uint(amount),
    ]));

    Transaction {
        to: adapter.to_string(),
        data: format!("0x{}", hex::encode(&calldata)),
        value: "0".to_string(),
    }
}

/// Convenience: split pUSD into outcome tokens for a regular (binary) V2 market.
///
/// Targets the `CtfCollateralAdapter`. Requires the caller to have approved
/// pUSD to the adapter (see `approve_pusd_for_ctf_adapter`).
pub fn split_pusd(condition_id: [u8; 32], partition: &[u64], amount: U256) -> Transaction {
    split_position_via_adapter(
        contracts::CTF_COLLATERAL_ADAPTER,
        contracts::PUSD,
        [0u8; 32],
        condition_id,
        partition,
        amount,
    )
}

/// Convenience: merge outcome tokens back to pUSD for a regular (binary) V2 market.
///
/// Targets the `CtfCollateralAdapter`. Requires the caller to have called
/// `setApprovalForAll(adapter, true)` on the CTF contract (see
/// `approve_ctf_for_ctf_adapter`).
pub fn merge_pusd(condition_id: [u8; 32], partition: &[u64], amount: U256) -> Transaction {
    merge_positions_via_adapter(
        contracts::CTF_COLLATERAL_ADAPTER,
        contracts::PUSD,
        [0u8; 32],
        condition_id,
        partition,
        amount,
    )
}

/// Convenience: split pUSD into outcome tokens for a neg-risk V2 market.
///
/// Targets the `NegRiskCtfCollateralAdapter`.
pub fn split_pusd_neg_risk(
    condition_id: [u8; 32],
    partition: &[u64],
    amount: U256,
) -> Transaction {
    split_position_via_adapter(
        contracts::NEG_RISK_CTF_COLLATERAL_ADAPTER,
        contracts::PUSD,
        [0u8; 32],
        condition_id,
        partition,
        amount,
    )
}

/// Convenience: merge outcome tokens back to pUSD for a neg-risk V2 market.
///
/// Targets the `NegRiskCtfCollateralAdapter`.
pub fn merge_pusd_neg_risk(
    condition_id: [u8; 32],
    partition: &[u64],
    amount: U256,
) -> Transaction {
    merge_positions_via_adapter(
        contracts::NEG_RISK_CTF_COLLATERAL_ADAPTER,
        contracts::PUSD,
        [0u8; 32],
        condition_id,
        partition,
        amount,
    )
}
