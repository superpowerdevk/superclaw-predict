use ethers::types::Address;
use std::str::FromStr;

// Polygon Mainnet (Chain ID 137)

/// USDC.e on Polygon
pub const USDC_E: &str = "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174";

/// pUSD (Polymarket USD) — V2 collateral, ERC-20 wrapper around USDC. 6 decimals.
/// Replaces USDC.e as the user-facing collateral after the V2 migration (2026-04-28).
pub const PUSD: &str = "0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB";

/// Conditional Tokens Framework
pub const CTF: &str = "0x4D97DCd97eC945f40cF65F87097ACe5EA0476045";

/// CTF Exchange (V1 — legacy USDC.e-collateralized)
pub const CTF_EXCHANGE: &str = "0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E";

/// Neg Risk CTF Exchange (V1 — legacy USDC.e-collateralized)
pub const NEG_RISK_EXCHANGE: &str = "0xC5d563A36AE78145C45a50134d48A1215220f80a";

/// CTF Exchange V2 (pUSD-collateralized; supports EIP-1271 smart-contract wallets).
pub const CTF_EXCHANGE_V2: &str = "0xE111180000d2663C0091e4f400237545B87B996B";

/// Neg Risk CTF Exchange V2 (pUSD-collateralized; supports EIP-1271).
pub const NEG_RISK_EXCHANGE_V2: &str = "0xe2222d279d744050d28e00520010520000310F59";

/// Neg Risk Adapter (used for redeeming neg-risk positions; unchanged in V2).
pub const NEG_RISK_ADAPTER: &str = "0xd91E80cF2E7be2e162c6513ceD06f1dD0dA35296";

/// CtfCollateralAdapter — V2 thin adapter that lets users split/merge/redeem
/// **using pUSD directly**. Internally unwraps pUSD → USDC.e, calls CTF, and
/// wraps released USDC.e back to pUSD before returning it to the caller.
/// Function signatures match the underlying CTF (splitPosition / mergePositions /
/// redeemPositions), so the existing operations work against this adapter too.
pub const CTF_COLLATERAL_ADAPTER: &str = "0xAdA100Db00Ca00073811820692005400218FcE1f";

/// NegRiskCtfCollateralAdapter — V2 pUSD-aware adapter for neg-risk markets.
pub const NEG_RISK_CTF_COLLATERAL_ADAPTER: &str = "0xadA2005600Dec949baf300f4C6120000bDB6eAab";

/// CollateralOnramp — wraps USDC / USDC.e into pUSD.
pub const COLLATERAL_ONRAMP: &str = "0x93070a847efEf7F70739046A929D47a521F5B8ee";

/// CollateralOfframp — unwraps pUSD back to USDC.e / USDC.
pub const COLLATERAL_OFFRAMP: &str = "0x2957922Eb93258b93368531d39fAcCA3B4dC5854";

/// Proxy Factory (for Proxy wallet type)
pub const PROXY_FACTORY: &str = "0xaB45c5A4B0c941a2F231C04C3f49182e1A254052";

/// Safe Factory (for Safe wallet type)
pub const SAFE_FACTORY: &str = "0xaacFeEa03eb1561C4e67d661e40682Bd20E3541b";

/// Safe Multisend contract
pub const SAFE_MULTISEND: &str = "0xA238CBeb142c10Ef7Ad8442C6D1f9E89e07e7761";

/// Relay Hub (for Proxy transactions)
pub const RELAY_HUB: &str = "0xD216153c06E857cD7f72665E0aF1d7D82172F494";

/// Default relayer URL
pub const RELAYER_URL: &str = "https://relayer-v2.polymarket.com/";

/// Zero address
pub const ZERO_ADDRESS: &str = "0x0000000000000000000000000000000000000000";

/// Safe init code hash for CREATE2 derivation
pub const SAFE_INIT_CODE_HASH: &str =
    "0x2bce2127ff07fb632d16c8347c4ebf501f4841168bed00d9e6ef715ddb6fcecf";

/// Proxy init code hash for CREATE2 derivation
pub const PROXY_INIT_CODE_HASH: &str =
    "0xd21df8dc65880a8606f09fe0ce3df9b8869287ab0b058be05aa9e8af6330a00b";

// ── V2 Deposit Wallet (POLY_1271) ──────────────────────────────────

/// DepositWallet factory — V2 (Polygon mainnet & Amoy testnet, same address).
///
/// Deposit Wallets are UUPS-upgradeable ERC-1967 proxies created via Solady's
/// `LibClone.cloneDeterministic_PUSH0`. Each owner gets a deterministic
/// address derived with CREATE2 (see [`crate::builder::derive::derive_deposit_wallet_address`]).
pub const DEPOSIT_WALLET_FACTORY: &str = "0x00000000000Fb5C9ADea0298D729A0CB3823Cc07";

/// DepositWallet implementation contract on **Polygon mainnet** (chain 137).
pub const DEPOSIT_WALLET_IMPLEMENTATION: &str = "0x58CA52ebe0DadfdF531Cde7062e76746de4Db1eB";

/// DepositWallet implementation contract on **Polygon Amoy** (chain 80002).
pub const DEPOSIT_WALLET_IMPLEMENTATION_AMOY: &str =
    "0x50a88fE9a441cB4c9c2aD6A2207CE2795C7D7Fbd";

/// Return the DepositWallet implementation address for a given chain id.
pub fn deposit_wallet_implementation(chain_id: u64) -> crate::error::Result<&'static str> {
    match chain_id {
        137 => Ok(DEPOSIT_WALLET_IMPLEMENTATION),
        80002 => Ok(DEPOSIT_WALLET_IMPLEMENTATION_AMOY),
        other => Err(crate::error::RelayerError::Other(format!(
            "DepositWallet implementation unknown for chain {other}; only 137 and 80002 supported"
        ))),
    }
}

// EIP-712 domain constants for DepositWallet batch signatures.
pub const DEPOSIT_WALLET_DOMAIN_NAME: &str = "DepositWallet";
pub const DEPOSIT_WALLET_DOMAIN_VERSION: &str = "1";

// Solady `LibClone.initCodeHashERC1967(implementation, args)` constants. The
// hashed pre-image layout is:
//   prefix(10) | implementation(20) | 0x6009(2) | const2(32) | const1(32) | args(n)
//
// `prefix` starts as `ERC1967_INIT_CODE_PREFIX` and the second byte (the 0x00
// slot) is replaced by `n = args.len()`. See
// https://github.com/Vectorized/solady/blob/main/src/utils/LibClone.sol

/// Bytes 0..10 of the Solady ERC-1967 init code, before patching in the args
/// length. Byte index 1 (the `0x00` slot) is replaced with `n = args.len()`.
pub const ERC1967_INIT_CODE_PREFIX: [u8; 10] = [
    0x61, 0x00, 0x3d, 0x3d, 0x81, 0x60, 0x23, 0x3d, 0x39, 0x73,
];

/// First constant suffix in the Solady ERC-1967 init code (32 bytes).
pub const ERC1967_CONST2: [u8; 32] = [
    0x51, 0x55, 0xf3, 0x36, 0x3d, 0x3d, 0x37, 0x3d, 0x3d, 0x36, 0x3d, 0x7f, 0x36, 0x08, 0x94, 0xa1,
    0x3b, 0xa1, 0xa3, 0x21, 0x06, 0x67, 0xc8, 0x28, 0x49, 0x2d, 0xb9, 0x8d, 0xca, 0x3e, 0x20, 0x76,
];

/// Second constant suffix in the Solady ERC-1967 init code (32 bytes).
pub const ERC1967_CONST1: [u8; 32] = [
    0xcc, 0x37, 0x35, 0xa9, 0x20, 0xa3, 0xca, 0x50, 0x5d, 0x38, 0x2b, 0xbc, 0x54, 0x5a, 0xf4, 0x3d,
    0x60, 0x00, 0x80, 0x3e, 0x60, 0x38, 0x57, 0x3d, 0x60, 0x00, 0xfd, 0x5b, 0x3d, 0x60, 0x00, 0xf3,
];

/// 2-byte separator inserted between the implementation address and `const2`.
pub const ERC1967_MID: [u8; 2] = [0x60, 0x09];

/// Parse a hex address string to an ethers Address.
pub fn parse_address(s: &str) -> crate::error::Result<Address> {
    Address::from_str(s).map_err(|e| crate::error::RelayerError::InvalidAddress(e.to_string()))
}

// ── ABI Function Selectors ──

/// ERC20 approve(address,uint256) selector
pub const APPROVE_SELECTOR: [u8; 4] = [0x09, 0x5e, 0xa7, 0xb3];

/// ERC1155 setApprovalForAll(address,bool) selector
pub const SET_APPROVAL_FOR_ALL_SELECTOR: [u8; 4] = [0xa2, 0x2c, 0xb4, 0x65];

/// ConditionalTokens redeemPositions(address,bytes32,bytes32,uint256[]) selector
pub const REDEEM_POSITIONS_SELECTOR: [u8; 4] = [0x01, 0xb7, 0x03, 0x7c]; // Will compute

/// ConditionalTokens splitPosition(address,bytes32,bytes32,uint256[],uint256) selector
pub const SPLIT_POSITION_SELECTOR: [u8; 4] = [0x72, 0xce, 0x42, 0x75]; // Will compute

/// ConditionalTokens mergePositions(address,bytes32,bytes32,uint256[],uint256) selector
pub const MERGE_POSITIONS_SELECTOR: [u8; 4] = [0xd3, 0x7b, 0xf4, 0x2e]; // Will compute

/// Safe multiSend(bytes) selector
pub const MULTISEND_SELECTOR: [u8; 4] = [0x8d, 0x80, 0xff, 0x0a];
