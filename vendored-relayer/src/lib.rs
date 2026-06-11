pub mod auth;
pub mod builder;
pub mod client;
pub mod contracts;
pub mod direct;
pub mod eip1271;
pub mod error;
pub mod operations;
pub mod types;

// Re-export key types for convenience.
pub use auth::{AuthMethod, BuilderConfig};
pub use client::{RelayClient, TransactionResponseHandle};
pub use direct::{DirectExecutor, DirectTxResult};
pub use error::{RelayerError, Result};
pub use operations::{
    approve, approve_ctf_for_ctf_adapter, approve_ctf_for_ctf_exchange,
    approve_ctf_for_ctf_exchange_v2, approve_ctf_for_neg_risk_adapter,
    approve_ctf_for_neg_risk_ctf_adapter, approve_ctf_for_neg_risk_exchange,
    approve_ctf_for_neg_risk_exchange_v2, approve_pusd_for_ctf_adapter,
    approve_pusd_for_ctf_exchange_v2, approve_pusd_for_neg_risk_ctf_adapter,
    approve_pusd_for_neg_risk_exchange_v2, approve_usdc_for_ctf_exchange,
    approve_usdc_for_neg_risk_exchange, merge_positions, merge_positions_via_adapter, merge_pusd,
    merge_pusd_neg_risk, merge_regular, redeem_neg_risk_positions, redeem_positions, redeem_regular,
    set_approval_for_all, split_position, split_position_via_adapter, split_pusd,
    split_pusd_neg_risk, split_regular,
};
pub use types::{
    DepositWalletCall, DepositWalletParams, RelayerTxType, Transaction, TxResult, TxState,
};
