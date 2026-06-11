//! EIP-1271 — smart-contract-wallet signature verification.
//!
//! After the Polymarket V2 migration (2026-04-28), both `CTFExchangeV2`
//! (`0xE111180000d2663C0091e4f400237545B87B996B`) and
//! `NegRiskCTFExchangeV2` (`0xe2222d279d744050d28e00520010520000310F59`)
//! verify orders from smart-contract wallets via `isValidSignature(bytes32,bytes)`.
//!
//! For the relayer-side flows in this SDK (split / merge / redeem / approve),
//! EIP-1271 is **not on the critical path** — those go through
//! `Safe.execTransaction` (EIP-712 owner signature) or the Polymarket proxy's
//! `proxy(...)` entrypoint and never produce an order signature.
//!
//! This module exists for the adjacent use-case: if you want to sign an
//! orderbook order with a Safe wallet, and have V2's exchange accept it via
//! EIP-1271, use [`sign_hash_for_safe`] to produce the signature bytes and
//! [`is_valid_signature_calldata`] to encode the verification call.
//!
//! Gnosis Safe v1.3.0 (the Polymarket Safe build) implements
//! `isValidSignature` and accepts a standard owner ECDSA signature
//! `r(32) || s(32) || v(1)` with `v ∈ {27, 28}`, which is what these helpers
//! produce.

use ethers::abi::{encode, Token};
use ethers::signers::LocalWallet;
use ethers::types::{Address, H256};
use ethers::utils::keccak256;

use crate::error::{RelayerError, Result};

/// Magic value returned by a successful `isValidSignature(bytes32,bytes)` call.
///
/// `bytes4(keccak256("isValidSignature(bytes32,bytes)"))` — the canonical
/// EIP-1271 v2 magic value (the older 0x20c13b0b form was for the deprecated
/// `isValidSignature(bytes,bytes)` overload and is not used by V2 contracts).
pub const EIP1271_MAGIC_VALUE: [u8; 4] = [0x16, 0x26, 0xba, 0x7e];

/// Sign a raw 32-byte hash with the Safe's owner EOA and pack the signature
/// in the format Safe's `isValidSignature` expects.
///
/// Output layout: `r (32) || s (32) || v (1)` with `v ∈ {27, 28}`.
///
/// The hash is **not** re-prefixed — pass the final EIP-712 hash (e.g. a V2
/// order hash) directly. Uses `LocalWallet::sign_hash`, which signs the raw
/// 32 bytes without the `"\x19Ethereum Signed Message:\n32"` prefix that
/// would otherwise break verification on the contract side.
pub fn sign_hash_for_safe(wallet: &LocalWallet, hash: H256) -> Result<Vec<u8>> {
    let sig = wallet
        .sign_hash(hash)
        .map_err(|e| RelayerError::Signing(e.to_string()))?;
    let mut bytes = Vec::with_capacity(65);
    let mut r = [0u8; 32];
    sig.r.to_big_endian(&mut r);
    bytes.extend_from_slice(&r);
    let mut s = [0u8; 32];
    sig.s.to_big_endian(&mut s);
    bytes.extend_from_slice(&s);
    // `LocalWallet::sign_hash` returns v ∈ {27, 28}, but normalise defensively.
    let v = sig.v as u8;
    let v_final = if v < 27 { v + 27 } else { v };
    bytes.push(v_final);
    Ok(bytes)
}

/// Encode a call to `isValidSignature(bytes32 hash, bytes signature)` on an
/// EIP-1271 wallet contract.
///
/// The returned bytes can be sent via `eth_call` to the smart-contract wallet
/// (e.g. a Safe) and the 4-byte response compared against
/// [`EIP1271_MAGIC_VALUE`] using [`is_magic_value`].
pub fn is_valid_signature_calldata(hash: H256, signature: &[u8]) -> Vec<u8> {
    let selector = &keccak256(b"isValidSignature(bytes32,bytes)")[..4];
    let mut out = selector.to_vec();
    out.extend_from_slice(&encode(&[
        Token::FixedBytes(hash.as_bytes().to_vec()),
        Token::Bytes(signature.to_vec()),
    ]));
    out
}

/// Check whether an `eth_call` response to `isValidSignature` indicates a
/// valid signature. Returns `true` only if the first 4 bytes equal
/// [`EIP1271_MAGIC_VALUE`].
pub fn is_magic_value(response: &[u8]) -> bool {
    response.len() >= 4 && response[..4] == EIP1271_MAGIC_VALUE
}

/// Build a Safe "pre-validated" signature for an owner that has already
/// approved a hash on-chain via `approveHash(hash)`.
///
/// Safe's signature decoder treats `v == 1` as "pre-validated by the owner
/// whose address is in the `r` slot". Encoding: `r = owner (left-padded to 32)`,
/// `s = 0`, `v = 1`. Useful when the relayer cannot produce a fresh ECDSA
/// signature (e.g. hardware-wallet or multi-owner flows).
pub fn safe_prevalidated_signature(owner: Address) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(65);
    let mut r = [0u8; 32];
    r[12..32].copy_from_slice(owner.as_bytes());
    bytes.extend_from_slice(&r);
    bytes.extend_from_slice(&[0u8; 32]); // s
    bytes.push(1u8); // v
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethers::signers::Signer;

    #[test]
    fn magic_value_matches_eip1271() {
        let h = keccak256(b"isValidSignature(bytes32,bytes)");
        assert_eq!(&h[..4], &EIP1271_MAGIC_VALUE);
    }

    #[test]
    fn is_magic_value_recognises_padded_response() {
        let mut padded = [0u8; 32];
        padded[..4].copy_from_slice(&EIP1271_MAGIC_VALUE);
        assert!(is_magic_value(&padded));
        assert!(!is_magic_value(&[0u8; 32]));
    }

    #[test]
    fn prevalidated_signature_layout() {
        let owner: Address = "0x000000000000000000000000000000000000abcd"
            .parse()
            .unwrap();
        let sig = safe_prevalidated_signature(owner);
        assert_eq!(sig.len(), 65);
        assert_eq!(&sig[12..32], owner.as_bytes());
        assert_eq!(&sig[32..64], &[0u8; 32]);
        assert_eq!(sig[64], 1);
    }

    #[test]
    fn sign_hash_for_safe_produces_65_bytes_with_v27_or_v28() {
        // Deterministic key — any 32-byte value works.
        let wallet: LocalWallet = "0x0000000000000000000000000000000000000000000000000000000000000001"
            .parse()
            .unwrap();
        let hash = H256::from(keccak256(b"some-order-hash"));
        let sig = sign_hash_for_safe(&wallet, hash).unwrap();
        assert_eq!(sig.len(), 65);
        assert!(matches!(sig[64], 27 | 28));
        // Sanity: signer is the expected EOA (we don't care about the exact
        // address, just that this compiles against the LocalWallet API).
        let _ = wallet.address();
    }
}
