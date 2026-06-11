use serde::{Deserialize, Serialize};

/// A transaction to be relayed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    /// Target contract address (checksummed hex).
    pub to: String,
    /// Encoded function calldata (hex with 0x prefix).
    pub data: String,
    /// Native token value to send (usually "0").
    pub value: String,
}

/// Wallet type for relayed transactions.
///
/// Maps to Polymarket's `signature_type` values:
///   0 = EOA (direct wallet, no abstraction)
///   1 = POLY_PROXY (magic.link proxy wallet)
///   2 = POLY_GNOSIS_SAFE (Gnosis Safe wallet)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RelayerTxType {
    /// EOA wallet — direct signing, no wallet abstraction (signature_type=0).
    Eoa = 0,
    /// Proxy wallet — auto-deploys on first tx (signature_type=1, e.g. magic.link).
    Proxy = 1,
    /// Gnosis Safe wallet — must call deploy() before first tx (signature_type=2).
    Safe = 2,
}

impl RelayerTxType {
    pub fn as_str(&self) -> &'static str {
        match self {
            RelayerTxType::Eoa => "EOA",
            RelayerTxType::Proxy => "PROXY",
            RelayerTxType::Safe => "SAFE",
        }
    }

    /// The numeric signature_type used by the Polymarket API.
    pub fn signature_type(&self) -> u8 {
        *self as u8
    }

    /// Parse from Polymarket's numeric signature_type.
    pub fn from_signature_type(sig_type: u8) -> Option<Self> {
        match sig_type {
            0 => Some(RelayerTxType::Eoa),
            1 => Some(RelayerTxType::Proxy),
            2 => Some(RelayerTxType::Safe),
            _ => None,
        }
    }
}

/// Transaction state in the relayer pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TxState {
    New,
    Executed,
    Mined,
    Confirmed,
    Failed,
    Invalid,
}

impl TxState {
    pub fn is_terminal(&self) -> bool {
        matches!(self, TxState::Confirmed | TxState::Failed | TxState::Invalid)
    }

    pub fn is_success(&self) -> bool {
        matches!(self, TxState::Mined | TxState::Confirmed)
    }
}

/// Result of a relayed transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayerTransactionResponse {
    #[serde(rename = "transactionID", alias = "transactionId")]
    pub transaction_id: String,
    pub state: String,
    #[serde(default)]
    pub hash: Option<String>,
    #[serde(default, rename = "transactionHash")]
    pub transaction_hash: Option<String>,
}

/// Parsed transaction result.
#[derive(Debug, Clone)]
pub struct TxResult {
    pub state: TxState,
    pub tx_hash: Option<String>,
    pub proxy_address: Option<String>,
    pub error: Option<String>,
}

/// Signature parameters for Safe transactions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SafeSignatureParams {
    pub gas_price: String,
    pub operation: String,
    pub safe_txn_gas: String,
    pub base_gas: String,
    pub gas_token: String,
    pub refund_receiver: String,
}

impl Default for SafeSignatureParams {
    fn default() -> Self {
        Self {
            gas_price: "0".to_string(),
            operation: "0".to_string(),
            safe_txn_gas: "0".to_string(),
            base_gas: "0".to_string(),
            gas_token: "0x0000000000000000000000000000000000000000".to_string(),
            refund_receiver: "0x0000000000000000000000000000000000000000".to_string(),
        }
    }
}

/// Signature parameters for Proxy transactions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxySignatureParams {
    pub gas_price: String,
    pub gas_limit: String,
    pub relayer_fee: String,
    pub relay_hub: String,
    pub relay: String,
}

/// Signature parameters for Safe-Create transactions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSignatureParams {
    pub payment_token: String,
    pub payment: String,
    pub payment_receiver: String,
}

impl Default for CreateSignatureParams {
    fn default() -> Self {
        Self {
            payment_token: "0x0000000000000000000000000000000000000000".to_string(),
            payment: "0".to_string(),
            payment_receiver: "0x0000000000000000000000000000000000000000".to_string(),
        }
    }
}

/// The full request body for POST /submit.
///
/// Shape varies per `tx_type`:
///   * `SAFE` / `PROXY` — populate `data`, `signature`, `signature_params`
///   * `SAFE-CREATE` — same but with `data="0x"` and a `CreateSignatureParams`
///   * `WALLET` — populate `signature` + `deposit_wallet_params`; `data` and
///     `signature_params` are omitted (calls live inside `depositWalletParams`)
///   * `WALLET-CREATE` — only `tx_type`, `from`, `to` are needed
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionRequest {
    #[serde(rename = "type")]
    pub tx_type: String,
    pub from: String,
    pub to: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_wallet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature_params: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deposit_wallet_params: Option<DepositWalletParams>,
}

/// A single call dispatched by a Deposit Wallet inside a `Batch`.
///
/// Mirrors the TypeScript SDK's `DepositWalletCall`. Note the field name is
/// `target` (matching the EIP-712 typed-data field), not `to`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepositWalletCall {
    pub target: String,
    pub value: String,
    pub data: String,
}

impl DepositWalletCall {
    /// Construct a `DepositWalletCall` with a zero `value`.
    pub fn new(target: impl Into<String>, data: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            value: "0".to_string(),
            data: data.into(),
        }
    }
}

impl From<Transaction> for DepositWalletCall {
    /// Adapt an internal `Transaction` (`{to, data, value}`) into a Deposit
    /// Wallet call (`{target, value, data}`).
    fn from(tx: Transaction) -> Self {
        Self {
            target: tx.to,
            value: tx.value,
            data: tx.data,
        }
    }
}

/// Body of the `depositWalletParams` field for a `type: "WALLET"` submit.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DepositWalletParams {
    pub deposit_wallet: String,
    pub deadline: String,
    pub calls: Vec<DepositWalletCall>,
}

/// Relay payload returned by GET /relay-payload.
#[derive(Debug, Clone, Deserialize)]
pub struct RelayPayload {
    pub address: String,
    pub nonce: String,
}
