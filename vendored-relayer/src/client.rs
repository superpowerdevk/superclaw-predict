use crate::auth::AuthMethod;
use crate::builder::{create, deposit_wallet, derive, proxy, safe};
use crate::contracts;
use crate::error::{RelayerError, Result};
use crate::types::*;
use ethers::signers::{LocalWallet, Signer};
use reqwest::Client;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{debug, info, warn};

/// Gas limit for Proxy transactions via Relay Hub.
///
/// The Relay Hub requires `gasleft() >= gasLimit` before calling the inner
/// function. The relayer bot has a fixed gas budget per transaction (~200-300K).
/// Setting this too high causes "Not enough gasleft()" reverts.
///
/// A single redeemPositions uses ~100-150K gas. Set low to stay within
/// the relayer bot's budget. The Polymarket frontend uses ~150K.
const DEFAULT_PROXY_GAS_LIMIT: u64 = 200_000;
const POLL_INTERVAL: Duration = Duration::from_secs(2);
const MAX_POLL_ATTEMPTS: u32 = 100;

/// Client for interacting with the Polymarket Builder Relayer.
#[derive(Clone)]
pub struct RelayClient {
    http: Client,
    base_url: String,
    chain_id: u64,
    signer: Arc<LocalWallet>,
    auth: AuthMethod,
    tx_type: RelayerTxType,
    /// Optional RPC URL for reading nonce on-chain (recommended for Safe wallets).
    rpc_url: Option<String>,
}

impl RelayClient {
    /// Create a new RelayClient.
    ///
    /// # Arguments
    /// * `chain_id` - Chain ID (137 for Polygon mainnet)
    /// * `signer` - Ethers LocalWallet for signing transactions
    /// * `auth` - Authentication method (Builder or RelayerKey)
    /// * `tx_type` - Wallet type (Safe or Proxy)
    pub async fn new(
        chain_id: u64,
        signer: LocalWallet,
        auth: AuthMethod,
        tx_type: RelayerTxType,
    ) -> Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;

        Ok(Self {
            http,
            base_url: contracts::RELAYER_URL.trim_end_matches('/').to_string(),
            chain_id,
            signer: Arc::new(signer),
            auth,
            tx_type,
            rpc_url: None,
        })
    }

    /// Set a custom relayer URL.
    pub fn set_url(&mut self, url: String) {
        self.base_url = url.trim_end_matches('/').to_string();
    }

    /// Set an RPC URL for reading the Safe nonce on-chain.
    ///
    /// **Highly recommended** — the relayer API `/nonce` endpoint can return
    /// stale values (e.g., 0), causing GS026 "Invalid owner" errors because
    /// the EIP-712 hash is computed with the wrong nonce.
    ///
    /// When set, `get_nonce()` reads the nonce directly from the Safe contract
    /// on-chain, falling back to the relayer API only on failure.
    pub fn set_rpc_url(&mut self, url: String) {
        self.rpc_url = Some(url);
    }

    /// Get the signer's EOA address.
    pub fn signer_address(&self) -> ethers::types::Address {
        self.signer.address()
    }

    /// Get the derived wallet address (Safe, Proxy, or EOA).
    pub fn wallet_address(&self) -> Result<ethers::types::Address> {
        match self.tx_type {
            RelayerTxType::Eoa => Ok(self.signer.address()),
            RelayerTxType::Safe => derive::derive_safe_address(self.signer.address()),
            RelayerTxType::Proxy => derive::derive_proxy_address(self.signer.address()),
        }
    }

    /// Check if the Safe wallet is deployed.
    pub async fn is_deployed(&self) -> Result<bool> {
        let wallet = self.wallet_address()?;
        let url = format!("{}/deployed?address={:?}", self.base_url, wallet);
        let resp = self.http.get(&url).send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(RelayerError::Api { status, message: body });
        }
        let text = resp.text().await?;
        let body: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| RelayerError::Other(format!("Parse Error on {}: {}", text, e)))?;
        // Handle multiple response formats:
        //   true / false                → bare bool
        //   "true" / "false"            → string
        //   {"deployed": true}          → object
        Ok(body.as_bool()
            .or_else(|| body.as_str().map(|s| s == "true"))
            .or_else(|| body.get("deployed").and_then(|v| v.as_bool()))
            .unwrap_or(false))
    }

    /// Get the current nonce for the wallet.
    ///
    /// For Safe wallets: reads from on-chain `nonce()` if `rpc_url` is set,
    /// otherwise falls back to the relayer API. The relayer API is known to
    /// return stale nonces (e.g., 0) which causes GS026 errors.
    pub async fn get_nonce(&self) -> Result<u64> {
        // For Safe wallets, prefer on-chain nonce if RPC URL is available
        if self.tx_type == RelayerTxType::Safe {
            if let Some(ref rpc_url) = self.rpc_url {
                match self.read_safe_nonce_onchain(rpc_url).await {
                    Ok(nonce) => {
                        debug!(nonce, source = "on-chain", "Safe nonce");
                        return Ok(nonce);
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to read on-chain nonce, falling back to relayer API");
                    }
                }
            }
        }

        // Fallback: relayer API
        let nonce = self.get_nonce_from_relayer().await?;
        debug!(nonce, source = "relayer-api", "Nonce");
        Ok(nonce)
    }

    /// Read Safe nonce directly from on-chain via JSON-RPC eth_call.
    async fn read_safe_nonce_onchain(&self, rpc_url: &str) -> Result<u64> {
        let safe_address = self.wallet_address()?;

        // nonce() selector = keccak256("nonce()")[..4]
        let selector = &ethers::utils::keccak256(b"nonce()")[..4];
        let calldata = format!("0x{}", hex::encode(selector));

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{
                "to": format!("{:?}", safe_address),
                "data": calldata,
            }, "latest"],
            "id": 1
        });

        let resp = self
            .http
            .post(rpc_url)
            .json(&body)
            .send()
            .await
            .map_err(|e| RelayerError::Other(format!("RPC request failed: {e}")))?;

        let text = resp.text().await
            .map_err(|e| RelayerError::Other(format!("RPC response read failed: {e}")))?;

        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| RelayerError::Other(format!("RPC parse error on {}: {e}", text)))?;

        // Check for JSON-RPC error
        if let Some(error) = json.get("error") {
            let msg = error.get("message").and_then(|m| m.as_str()).unwrap_or("unknown");
            return Err(RelayerError::Other(format!("RPC error: {msg}")));
        }

        let result_hex = json.get("result")
            .and_then(|r| r.as_str())
            .ok_or_else(|| RelayerError::Other(format!("No result in RPC response: {text}")))?;

        // Parse hex result → u64
        let result_hex = result_hex.strip_prefix("0x").unwrap_or(result_hex);
        let nonce = u64::from_str_radix(result_hex, 16)
            .map_err(|e| RelayerError::Other(format!("Invalid nonce hex '{}': {e}", result_hex)))?;

        Ok(nonce)
    }

    /// Get nonce from the relayer API (may be stale for Safe wallets).
    async fn get_nonce_from_relayer(&self) -> Result<u64> {
        let url = format!(
            "{}/nonce?address={:?}&type={}",
            self.base_url,
            self.signer.address(),
            self.tx_type.as_str()
        );
        let resp = self.http.get(&url).send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(RelayerError::Api { status, message: body });
        }
        let text = resp.text().await?;
        debug!(raw_response = %text, "Relayer nonce response");

        let body: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| RelayerError::Other(format!("Nonce parse error on {}: {}", text, e)))?;
        let nonce = body
            .as_u64()
            .or_else(|| body.as_str().and_then(|s| s.parse().ok()))
            .unwrap_or(0);
        Ok(nonce)
    }

    /// Get relay payload (for Proxy transactions).
    async fn get_relay_payload(&self) -> Result<RelayPayload> {
        let url = format!(
            "{}/relay-payload?address={:?}&type=PROXY",
            self.base_url,
            self.signer.address()
        );
        let resp = self.http.get(&url).send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(RelayerError::Api { status, message: body });
        }
        let text = resp.text().await?;
        Ok(serde_json::from_str(&text).map_err(|e| RelayerError::Other(format!("Payload Parse Error on {}: {}", text, e)))?)
    }

    /// Get a transaction's status by ID.
    pub async fn get_transaction(&self, tx_id: &str) -> Result<TxResult> {
        let url = format!("{}/transaction?id={}", self.base_url, tx_id);
        let resp = self.http.get(&url).send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(RelayerError::Api { status, message: body });
        }
        let text = resp.text().await?;
        debug!(raw_response = %text, "Relayer get_transaction response");

        let data = parse_relayer_response(&text)?;
        let state = parse_tx_state(&data.state);

        // Extract error details from the raw response for failed transactions
        let error = if state == TxState::Failed || state == TxState::Invalid {
            extract_error_from_response(&text)
        } else {
            None
        };

        Ok(TxResult {
            state,
            tx_hash: data.transaction_hash.or(data.hash),
            proxy_address: None,
            error,
        })
    }

    /// Deploy a Safe wallet (one-time, Safe wallet type only).
    pub async fn deploy(&self) -> Result<TxResult> {
        if self.tx_type != RelayerTxType::Safe {
            return Err(RelayerError::Other(
                "deploy() is only for Safe wallet type".to_string(),
            ));
        }

        if self.is_deployed().await? {
            let wallet = self.wallet_address()?;
            return Err(RelayerError::WalletAlreadyDeployed(format!("{:?}", wallet)));
        }

        let safe_address = self.wallet_address()?;
        let (signature, params) =
            create::build_create_transaction(self.signer.as_ref(), self.chain_id).await?;

        let request = TransactionRequest {
            tx_type: "SAFE-CREATE".to_string(),
            from: format!("{:?}", self.signer.address()),
            to: contracts::SAFE_FACTORY.to_string(),
            proxy_wallet: Some(format!("{:?}", safe_address)),
            data: Some("0x".to_string()),
            signature: Some(signature),
            nonce: None,
            signature_params: Some(
                serde_json::to_value(&params).map_err(|e| RelayerError::Abi(e.to_string()))?,
            ),
            metadata: Some("Deploy Safe wallet".to_string()),
            value: Some("0".to_string()),
            deposit_wallet_params: None,
        };

        let response = self.submit(request).await?;
        info!(tx_id = %response.transaction_id, "Safe deploy submitted");

        let result = self.wait_for_tx(&response.transaction_id).await?;
        Ok(TxResult {
            proxy_address: Some(format!("{:?}", safe_address)),
            ..result
        })
    }

    /// Execute one or more transactions through the relayer.
    pub async fn execute(
        &self,
        txs: Vec<Transaction>,
        description: &str,
    ) -> Result<TransactionResponseHandle> {
        if txs.is_empty() {
            return Err(RelayerError::Other("No transactions to execute".to_string()));
        }

        let request = match self.tx_type {
            RelayerTxType::Eoa => {
                return Err(RelayerError::Other(
                    "EOA wallets cannot use the gasless relayer — send transactions directly".to_string(),
                ));
            }
            RelayerTxType::Safe => self.build_safe_request(&txs, description).await?,
            RelayerTxType::Proxy => self.build_proxy_request(&txs, description).await?,
        };

        let response = self.submit(request).await?;
        info!(tx_id = %response.transaction_id, description, "Transaction submitted");

        Ok(TransactionResponseHandle {
            tx_id: response.transaction_id,
            client: self.clone(),
        })
    }

    /// Execute multiple groups of transactions sequentially, waiting for each
    /// to confirm before submitting the next.
    ///
    /// This avoids nonce collisions when the relay bot (Gelato) cannot handle
    /// back-to-back requests for the same proxy wallet.
    ///
    /// # Arguments
    /// * `batches` - Each inner `Vec<Transaction>` is submitted as one relay request.
    /// * `delay` - Wait time between confirmed batches (default 5s if `None`).
    /// * `on_progress` - Optional callback `(completed, total)` after each batch confirms.
    ///
    /// Returns a vec of `TxResult`, one per batch (in order).
    pub async fn execute_sequential(
        &self,
        batches: Vec<Vec<Transaction>>,
        delay: Option<Duration>,
        on_progress: Option<&dyn Fn(usize, usize)>,
    ) -> Result<Vec<TxResult>> {
        let delay = delay.unwrap_or(Duration::from_secs(5));
        let total = batches.len();
        let mut results = Vec::with_capacity(total);

        for (i, txs) in batches.into_iter().enumerate() {
            let desc = format!("Batch {}/{}", i + 1, total);
            info!(batch = i + 1, total, "Submitting sequential batch");

            let handle = self.execute(txs, &desc).await?;
            let result = handle.wait().await?;
            results.push(result);

            if let Some(cb) = on_progress {
                cb(i + 1, total);
            }

            // Delay between batches (skip after the last one)
            if i + 1 < total {
                debug!(delay_secs = delay.as_secs(), "Waiting between batches");
                sleep(delay).await;
            }
        }

        Ok(results)
    }

    /// Execute multiple transactions as a single batched relay request.
    ///
    /// All transactions share one nonce and one relay call, completely avoiding
    /// nonce collisions. More gas-efficient than sequential execution.
    ///
    /// For Proxy wallets, the gas limit scales with the number of transactions:
    ///   `gas_limit = 150_000 + (extra_txs * 80_000)`, capped at 400_000.
    ///
    /// For Safe wallets, multiple transactions are packed via multiSend
    /// (already handled by `build_safe_request`).
    pub async fn execute_batch(
        &self,
        txs: Vec<Transaction>,
        description: &str,
    ) -> Result<TxResult> {
        if txs.is_empty() {
            return Err(RelayerError::Other("No transactions to batch".to_string()));
        }

        info!(count = txs.len(), description, "Submitting batch transaction");

        let request = match self.tx_type {
            RelayerTxType::Eoa => {
                return Err(RelayerError::Other(
                    "EOA wallets cannot use the gasless relayer".to_string(),
                ));
            }
            RelayerTxType::Safe => {
                // Safe already supports multisend natively
                self.build_safe_request(&txs, description).await?
            }
            RelayerTxType::Proxy => {
                // build_proxy_request now scales gas limit internally based on tx count.
                self.build_proxy_request(&txs, description).await?
            }
        };

        let response = self.submit(request).await?;
        info!(tx_id = %response.transaction_id, description, "Batch submitted");

        self.wait_for_tx(&response.transaction_id).await
    }



    /// Build a Safe transaction request with full EIP-712 signing.
    async fn build_safe_request(
        &self,
        txs: &[Transaction],
        metadata: &str,
    ) -> Result<TransactionRequest> {
        let safe_address = self.wallet_address()?;

        // Don't block on is_deployed() — the relayer will reject if not deployed.
        // This matches the Python SDK behavior.

        let nonce = self.get_nonce().await?;

        let (data, to, signature, sig_params) = safe::build_safe_transaction(
            self.signer.as_ref(),
            self.chain_id,
            safe_address,
            txs,
            nonce,
        )
        .await?;

        Ok(TransactionRequest {
            tx_type: "SAFE".to_string(),
            from: format!("{:?}", self.signer.address()),
            to: format!("{:?}", to),
            proxy_wallet: Some(format!("{:?}", safe_address)),
            data: Some(data),
            signature: Some(signature),
            nonce: Some(nonce.to_string()),
            signature_params: Some(
                serde_json::to_value(&sig_params)
                    .map_err(|e| RelayerError::Abi(e.to_string()))?,
            ),
            metadata: Some(metadata.to_string()),
            value: Some("0".to_string()),
            deposit_wallet_params: None,
        })
    }

    /// Build a Proxy transaction request with dynamic gas limit scaling.
    async fn build_proxy_request(
        &self,
        txs: &[Transaction],
        metadata: &str,
    ) -> Result<TransactionRequest> {
        let proxy_address = self.wallet_address()?;
        let relay_payload = self.get_relay_payload().await?;

        // Scale gas limit for multiple operations
        let extra = txs.len().saturating_sub(1) as u64;
        let gas_limit = (DEFAULT_PROXY_GAS_LIMIT + extra * 80_000).min(400_000);
        debug!(gas_limit, tx_count = txs.len(), "Dynamic proxy gas limit");

        let (data, signature, sig_params) = proxy::build_proxy_transaction(
            self.signer.as_ref(),
            self.signer.address(),
            txs,
            &relay_payload,
            gas_limit,
        )
        .await?;

        Ok(TransactionRequest {
            tx_type: "PROXY".to_string(),
            from: format!("{:?}", self.signer.address()),
            to: contracts::PROXY_FACTORY.to_string(),
            proxy_wallet: Some(format!("{:?}", proxy_address)),
            data: Some(data),
            signature: Some(signature),
            nonce: Some(relay_payload.nonce),
            signature_params: Some(
                serde_json::to_value(&sig_params)
                    .map_err(|e| RelayerError::Abi(e.to_string()))?,
            ),
            metadata: Some(metadata.to_string()),
            value: Some("0".to_string()),
            deposit_wallet_params: None,
        })
    }

    /// Submit a transaction request to the relayer.
    pub async fn submit(&self, request: TransactionRequest) -> Result<RelayerTransactionResponse> {
        let url = format!("{}/submit", self.base_url);
        let body = serde_json::to_string(&request)
            .map_err(|e| RelayerError::Abi(e.to_string()))?;

        debug!(url = %url, body_len = body.len(), "Submitting to relayer");

        let auth_headers = self.auth.headers("POST", "/submit", &body)?;

        debug!(
            headers = ?auth_headers.keys().map(|k| k.as_str()).collect::<Vec<_>>(),
            "Auth headers"
        );

        let resp = self
            .http
            .post(&url)
            .headers(auth_headers)
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let err = resp.text().await.unwrap_or_default();
            if status == 429 {
                return Err(RelayerError::QuotaExhausted);
            }
            return Err(RelayerError::Api { status, message: err });
        }

        let text = resp.text().await?;
        debug!(raw_response = %text, "Relayer submit response");

        parse_relayer_response(&text)
    }

    /// Poll for transaction confirmation.
    async fn wait_for_tx(&self, tx_id: &str) -> Result<TxResult> {
        for attempt in 0..MAX_POLL_ATTEMPTS {
            sleep(POLL_INTERVAL).await;
            let result = self.get_transaction(tx_id).await?;
            debug!(attempt, state = ?result.state, tx_id, "Polling transaction");

            if result.state.is_terminal() {
                let tx_hash_str = result.tx_hash.as_deref().unwrap_or("no hash");
                let error_str = result.error.as_deref().unwrap_or("no details");
                if result.state == TxState::Failed {
                    return Err(RelayerError::TransactionFailed(format!(
                        "Transaction {} failed | tx: {} | reason: {}",
                        tx_id, tx_hash_str, error_str
                    )));
                }
                if result.state == TxState::Invalid {
                    return Err(RelayerError::TransactionInvalid(format!(
                        "Transaction {} rejected | tx: {} | reason: {}",
                        tx_id, tx_hash_str, error_str
                    )));
                }
                return Ok(result);
            }
        }
        Err(RelayerError::Timeout)
    }

    // ── Convenience methods ──

    /// Approve USDC.e for CTF Exchange.
    pub async fn approve_usdc_for_ctf(&self) -> Result<TransactionResponseHandle> {
        let tx = crate::operations::approve_usdc_for_ctf_exchange();
        self.execute(vec![tx], "Approve USDC for CTF Exchange").await
    }

    /// Approve USDC.e for Neg Risk CTF Exchange.
    pub async fn approve_usdc_for_negrisk(&self) -> Result<TransactionResponseHandle> {
        let tx = crate::operations::approve_usdc_for_neg_risk_exchange();
        self.execute(vec![tx], "Approve USDC for NegRisk Exchange").await
    }

    /// Approve CTF tokens (ERC1155) for CTF Exchange.
    pub async fn approve_ctf_for_exchange(&self) -> Result<TransactionResponseHandle> {
        let tx = crate::operations::approve_ctf_for_ctf_exchange();
        self.execute(vec![tx], "Approve CTF for Exchange").await
    }

    /// Set up all standard approvals in a single batch.
    pub async fn setup_approvals(&self) -> Result<TransactionResponseHandle> {
        let txs = vec![
            crate::operations::approve_usdc_for_ctf_exchange(),
            crate::operations::approve_usdc_for_neg_risk_exchange(),
            crate::operations::approve_ctf_for_ctf_exchange(),
            crate::operations::approve_ctf_for_neg_risk_exchange(),
            crate::operations::approve_ctf_for_neg_risk_adapter(),
        ];
        self.execute(txs, "Setup all approvals").await
    }

    // ── V2 Deposit Wallet (POLY_1271) ───────────────────────────────

    /// Predict the V2 Deposit Wallet address for the current signer EOA.
    ///
    /// Pure CREATE2 derivation — does not query the relayer.
    pub fn derive_deposit_wallet_address(&self) -> Result<ethers::types::Address> {
        derive::derive_deposit_wallet_address_for_chain(self.signer.address(), self.chain_id)
    }

    /// Deploy a Deposit Wallet via the relayer (gasless, type `WALLET-CREATE`).
    ///
    /// Idempotent on the relayer side: a second call for an already-deployed
    /// wallet typically returns `Invalid`. Use [`is_deposit_wallet_deployed`]
    /// to gate the call.
    ///
    /// [`is_deposit_wallet_deployed`]: Self::is_deposit_wallet_deployed
    pub async fn deploy_deposit_wallet(&self) -> Result<TxResult> {
        let owner = self.signer.address();
        let request = deposit_wallet::build_create_request(owner);
        let response = self.submit(request).await?;
        info!(tx_id = %response.transaction_id, "Deposit wallet deploy submitted");
        self.wait_for_tx(&response.transaction_id).await
    }

    /// Check whether a Deposit Wallet is already deployed at the derived
    /// address, using `GET /deployed?address=...&type=WALLET`.
    pub async fn is_deposit_wallet_deployed(&self) -> Result<bool> {
        let wallet = self.derive_deposit_wallet_address()?;
        let url = format!("{}/deployed?address={:?}&type=WALLET", self.base_url, wallet);
        let resp = self.http.get(&url).send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(RelayerError::Api { status, message: body });
        }
        let text = resp.text().await?;
        let body: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| RelayerError::Other(format!("Parse Error on {}: {}", text, e)))?;
        Ok(body
            .as_bool()
            .or_else(|| body.as_str().map(|s| s == "true"))
            .or_else(|| body.get("deployed").and_then(|v| v.as_bool()))
            .unwrap_or(false))
    }

    /// Fetch the Deposit Wallet nonce from the relayer (`GET /nonce?type=WALLET`).
    pub async fn get_deposit_wallet_nonce(&self) -> Result<u64> {
        let url = format!(
            "{}/nonce?address={:?}&type=WALLET",
            self.base_url,
            self.signer.address()
        );
        let resp = self.http.get(&url).send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(RelayerError::Api { status, message: body });
        }
        let text = resp.text().await?;
        debug!(raw_response = %text, "Deposit wallet nonce response");
        let body: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| RelayerError::Other(format!("Nonce parse error on {}: {}", text, e)))?;
        let nonce = body
            .as_u64()
            .or_else(|| body.get("nonce").and_then(|v| v.as_u64()))
            .or_else(|| {
                body.get("nonce")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok())
            })
            .or_else(|| body.as_str().and_then(|s| s.parse().ok()))
            .ok_or_else(|| RelayerError::Other(format!("No nonce in response: {text}")))?;
        Ok(nonce)
    }

    /// Execute a batch of calls through the user's Deposit Wallet (V2 flow).
    ///
    /// Mirrors the TypeScript SDK's `executeDepositWalletBatch`:
    ///   1. Read the WALLET nonce from `/nonce?type=WALLET`.
    ///   2. EIP-712-sign the `Batch{wallet, nonce, deadline, calls[]}` struct.
    ///   3. POST `/submit` with `type: "WALLET"` and `depositWalletParams`.
    ///
    /// The Deposit Wallet must already be deployed (see
    /// [`deploy_deposit_wallet`]). If `deposit_wallet` is `None` the address
    /// is derived from the signer.
    ///
    /// `deadline` is a unix timestamp (seconds) after which the batch is
    /// invalid. The TS examples typically use `now + 4 minutes`.
    ///
    /// [`deploy_deposit_wallet`]: Self::deploy_deposit_wallet
    pub async fn execute_deposit_wallet_batch(
        &self,
        calls: Vec<DepositWalletCall>,
        deposit_wallet_addr: Option<ethers::types::Address>,
        deadline: u64,
        metadata: Option<&str>,
    ) -> Result<TransactionResponseHandle> {
        if calls.is_empty() {
            return Err(RelayerError::Other(
                "No calls to execute in deposit wallet batch".to_string(),
            ));
        }

        let owner = self.signer.address();
        let wallet_addr = match deposit_wallet_addr {
            Some(w) => w,
            None => self.derive_deposit_wallet_address()?,
        };

        let nonce = self.get_deposit_wallet_nonce().await?;

        let request = deposit_wallet::build_batch_request(
            self.signer.as_ref(),
            self.chain_id,
            owner,
            wallet_addr,
            nonce,
            deadline,
            calls,
            metadata.map(|s| s.to_string()),
        )?;

        let response = self.submit(request).await?;
        info!(
            tx_id = %response.transaction_id,
            wallet = ?wallet_addr,
            nonce,
            "Deposit wallet batch submitted"
        );

        Ok(TransactionResponseHandle {
            tx_id: response.transaction_id,
            client: self.clone(),
        })
    }

    /// Set up all V2 approvals (pUSD + adapters + V2 exchanges) in one batch.
    ///
    /// After the V2 migration (2026-04-28) the user-facing collateral is pUSD,
    /// so trading and split/merge require fresh approvals against the new
    /// contracts. Includes:
    ///   * pUSD → V2 CTF Exchange / V2 Neg Risk Exchange (for orderbook trades)
    ///   * pUSD → CtfCollateralAdapter / NegRiskCtfCollateralAdapter (for split)
    ///   * CTF (ERC-1155) → V2 exchanges + both collateral adapters (for trades / merge)
    ///   * CTF (ERC-1155) → NegRiskAdapter (for neg-risk redeem; unchanged from V1)
    ///
    /// Note: for **Proxy** wallets, this is 9 inner calls and will exceed the
    /// relayer's per-tx gas budget — call the individual approvals one by one
    /// or use `execute_sequential` instead. Safe wallets pack these via
    /// MultiSend and submit them as one transaction without issue.
    pub async fn setup_approvals_v2(&self) -> Result<TransactionResponseHandle> {
        let txs = vec![
            crate::operations::approve_pusd_for_ctf_exchange_v2(),
            crate::operations::approve_pusd_for_neg_risk_exchange_v2(),
            crate::operations::approve_pusd_for_ctf_adapter(),
            crate::operations::approve_pusd_for_neg_risk_ctf_adapter(),
            crate::operations::approve_ctf_for_ctf_exchange_v2(),
            crate::operations::approve_ctf_for_neg_risk_exchange_v2(),
            crate::operations::approve_ctf_for_ctf_adapter(),
            crate::operations::approve_ctf_for_neg_risk_ctf_adapter(),
            crate::operations::approve_ctf_for_neg_risk_adapter(),
        ];
        self.execute(txs, "Setup V2 approvals (pUSD)").await
    }
}

/// Handle for a submitted transaction, with polling support.
pub struct TransactionResponseHandle {
    pub tx_id: String,
    client: RelayClient,
}

impl TransactionResponseHandle {
    /// Poll the transaction until it reaches a terminal state.
    pub async fn wait(self) -> Result<TxResult> {
        self.client.wait_for_tx(&self.tx_id).await
    }

    /// Get the transaction ID.
    pub fn id(&self) -> &str {
        &self.tx_id
    }
}

/// Parse a relayer response that may be either:
///   - A flat `RelayerTransactionResponse` JSON object
///   - A JSON array containing the response object (e.g. `[{"transactionId": "..."}]`)
///   - A wrapper object with a nested transaction (e.g., `{"data": {...}}`)
fn parse_relayer_response(text: &str) -> Result<RelayerTransactionResponse> {
    // 1. Try direct deserialization
    if let Ok(resp) = serde_json::from_str::<RelayerTransactionResponse>(text) {
        return Ok(resp);
    }

    // 2. Try as a JSON value
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
        // Handle JSON array (take first element)
        if let Some(first) = value.as_array().and_then(|a| a.first()) {
            if let Ok(resp) = serde_json::from_value::<RelayerTransactionResponse>(first.clone()) {
                warn!("Relayer returned JSON array; extracted first element");
                return Ok(resp);
            }
            // If it's an array of wrappers/partial objects, continue searching inside the first element
            return parse_relayer_value(first);
        }

        return parse_relayer_value(&value);
    }

    Err(RelayerError::Other(format!(
        "Failed to parse relayer response: {}", text
    )))
}

/// Helper to parse a JSON value that might be a wrapped or partial relayer response.
fn parse_relayer_value(value: &serde_json::Value) -> Result<RelayerTransactionResponse> {
    // 1. Try common wrapper patterns: {"data": {...}}, {"result": {...}}, {"transaction": {...}}
    for key in &["data", "result", "transaction"] {
        if let Some(inner) = value.get(key) {
            if let Ok(resp) = serde_json::from_value::<RelayerTransactionResponse>(inner.clone()) {
                warn!(wrapper_key = key, "Relayer returned wrapped response");
                return Ok(resp);
            }
        }
    }

    // 2. Try extracting transactionId/transactionID from top-level (partial match)
    let tx_id = value.get("transactionId")
        .or_else(|| value.get("transactionID"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    if let Some(id) = tx_id {
        let state = value.get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("NEW")
            .to_string();
        let hash = value.get("hash")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let transaction_hash = value.get("transactionHash")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        warn!("Relayer response required manual field extraction");
        return Ok(RelayerTransactionResponse {
            transaction_id: id,
            state,
            hash,
            transaction_hash,
        });
    }

    Err(RelayerError::Other(format!(
        "Value is not a valid relayer response: {}", value
    )))
}

/// Parse a transaction state string from the relayer.
///
/// Handles both formats:
///   - Plain: "NEW", "MINED", "CONFIRMED", "FAILED", "INVALID"
///   - Prefixed: "STATE_NEW", "STATE_MINED", "STATE_CONFIRMED", etc.
fn parse_tx_state(s: &str) -> TxState {
    // Normalize: uppercase + strip "STATE_" prefix
    let normalized = s.to_uppercase();
    let key = normalized.strip_prefix("STATE_").unwrap_or(&normalized);
    match key {
        "NEW" => TxState::New,
        "EXECUTED" => TxState::Executed,
        "MINED" => TxState::Mined,
        "CONFIRMED" => TxState::Confirmed,
        "FAILED" => TxState::Failed,
        "INVALID" => TxState::Invalid,
        _ => {
            warn!(raw_state = s, "Unknown transaction state, treating as New");
            TxState::New
        }
    }
}

/// Extract error/reason from a raw relayer response JSON.
///
/// Looks for common error fields in the response or its array wrapper.
fn extract_error_from_response(text: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;

    // If it's an array, look inside the first element
    let obj = if let Some(first) = value.as_array().and_then(|a| a.first()) {
        first
    } else {
        &value
    };

    // Try common error field names
    for key in &["errorMsg", "error", "reason", "failureReason", "revertReason", "message", "statusMessage"] {
        if let Some(v) = obj.get(key) {
            let s = if let Some(s) = v.as_str() {
                s.to_string()
            } else {
                v.to_string()
            };
            if !s.is_empty() && s != "\"\"" && s != "null" {
                return Some(s);
            }
        }
    }

    // Try nested: derivedMetadata.error, etc.
    if let Some(meta) = obj.get("derivedMetadata") {
        for key in &["error", "reason", "revertReason"] {
            if let Some(v) = meta.get(key) {
                if let Some(s) = v.as_str() {
                    if !s.is_empty() {
                        return Some(s.to_string());
                    }
                }
            }
        }
    }

    None
}
