//! Native ETH uses integer gwei in the existing proof ledger. The oracle quote
//! is part of the proof-bound lease payload and is never refreshed at settlement.
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use zkapi_types::Felt252;

use crate::error::ServerError;

pub const UNITS_PER_ETH: u128 = 1_000_000_000;
pub const MAX_SAFE_UNITS: u128 = 9_007_199_254_740_991;

#[derive(Debug, Clone)]
pub struct NativeBillingConfig {
    pub rpc_url: String,
    pub feed_address: String,
    pub decimals: u8,
    pub max_age_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NativeBillingQuote {
    pub asset: String,
    pub units_per_eth: u64,
    pub chain_id: u64,
    pub feed_address: String,
    pub round_id: String,
    pub answer: String,
    pub decimals: u8,
    pub updated_at: u64,
    pub expires_at: u64,
}

fn invalid(message: &str) -> ServerError {
    ServerError::InvalidRequest(message.to_string())
}

fn decimal(value: &str) -> Result<u128, ServerError> {
    if value.is_empty()
        || value.len() > 39
        || !value.bytes().all(|b| b.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err(invalid("invalid native billing quote integer"));
    }
    value
        .parse()
        .map_err(|_| invalid("native billing quote integer overflow"))
}

impl NativeBillingQuote {
    pub fn validate_identity(
        &self,
        config: &NativeBillingConfig,
        chain_id: u64,
    ) -> Result<(), ServerError> {
        if self.asset != "native_eth"
            || self.units_per_eth != UNITS_PER_ETH as u64
            || self.chain_id != chain_id
            || self.feed_address != config.feed_address.to_lowercase()
            || self.decimals != config.decimals
            || self.decimals > 18
            || self.updated_at == 0
            || self.expires_at
                != self
                    .updated_at
                    .checked_add(config.max_age_seconds)
                    .ok_or_else(|| invalid("native billing quote expiry overflow"))?
        {
            return Err(invalid(
                "native billing quote does not match the pinned deployment",
            ));
        }
        let round = decimal(&self.round_id)?;
        let price = decimal(&self.answer)?;
        if round == 0 || round >= (1u128 << 80) || price == 0 || price > 1_000_000_000_000_000_000 {
            return Err(invalid("native billing quote is outside supported bounds"));
        }
        Ok(())
    }

    fn scale(&self) -> Result<u128, ServerError> {
        UNITS_PER_ETH
            .checked_mul(
                10u128
                    .checked_pow(self.decimals as u32)
                    .ok_or_else(|| invalid("native billing decimals overflow"))?,
            )
            .ok_or_else(|| invalid("native billing scale overflow"))
    }

    pub fn limit_micro_usd(&self, units: u128) -> Result<u128, ServerError> {
        if units == 0 || units > MAX_SAFE_UNITS {
            return Err(invalid(
                "native lease amount is outside browser-safe bounds",
            ));
        }
        let result = units
            .checked_mul(decimal(&self.answer)?)
            .and_then(|n| n.checked_mul(1_000_000))
            .ok_or_else(|| invalid("native billing budget overflow"))?
            / self.scale()?;
        if result == 0 || result > MAX_SAFE_UNITS {
            return Err(invalid(
                "native lease USD budget is outside supported bounds",
            ));
        }
        Ok(result)
    }

    pub fn charge_units(&self, micro_usd: u128) -> Result<u128, ServerError> {
        if micro_usd == 0 {
            return Ok(0);
        }
        let numerator = micro_usd
            .checked_mul(self.scale()?)
            .ok_or_else(|| invalid("native billing charge overflow"))?;
        let denominator = decimal(&self.answer)?
            .checked_mul(1_000_000)
            .ok_or_else(|| invalid("native billing price overflow"))?;
        if denominator == 0 {
            return Err(invalid("native billing price is zero"));
        }
        let value = numerator / denominator + u128::from(numerator % denominator != 0);
        if value > MAX_SAFE_UNITS {
            return Err(invalid(
                "native billing charge is outside browser-safe bounds",
            ));
        }
        Ok(value)
    }
}

pub struct NativeBillingOracle {
    config: NativeBillingConfig,
    chain_id: u64,
    contract_address: String,
    http: reqwest::Client,
}

impl NativeBillingOracle {
    pub fn new(
        config: NativeBillingConfig,
        chain_id: u64,
        contract_address: String,
    ) -> anyhow::Result<Self> {
        let url = reqwest::Url::parse(&config.rpc_url)?;
        anyhow::ensure!(
            url.scheme() == "https"
                || (url.scheme() == "http"
                    && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1"))),
            "native oracle RPC requires HTTPS or loopback HTTP"
        );
        anyhow::ensure!(
            url.username().is_empty() && url.password().is_none() && url.fragment().is_none(),
            "native oracle RPC cannot contain credentials or a fragment"
        );
        anyhow::ensure!(
            valid_address(&config.feed_address) && valid_address(&contract_address),
            "native oracle and vault require nonzero Ethereum addresses"
        );
        anyhow::ensure!(
            config.decimals <= 18 && config.max_age_seconds > 0 && config.max_age_seconds <= 86_400,
            "invalid native oracle decimals or freshness limit"
        );
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            config,
            chain_id,
            contract_address,
            http,
        })
    }

    async fn rpc(&self, method: &str, params: Value) -> Result<Value, ServerError> {
        let response = self
            .http
            .post(&self.config.rpc_url)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
            .send()
            .await
            .map_err(|_| invalid("native billing oracle RPC unavailable"))?;
        if !response.status().is_success() || response.content_length().is_some_and(|n| n > 65_536)
        {
            return Err(invalid("native billing oracle RPC rejected the read"));
        }
        let mut response = response;
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| invalid("native billing oracle RPC unavailable"))?
        {
            if body.len().saturating_add(chunk.len()) > 65_536 {
                return Err(invalid("native billing oracle RPC response too large"));
            }
            body.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&body)
            .map_err(|_| invalid("invalid native billing oracle RPC response"))?;
        if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
            || value.get("id").and_then(Value::as_u64) != Some(1)
        {
            return Err(invalid(
                "native billing oracle RPC response identity mismatch",
            ));
        }
        if value.get("error").is_some() {
            return Err(invalid("native billing oracle RPC call failed"));
        }
        value
            .get("result")
            .cloned()
            .ok_or_else(|| invalid("missing native billing oracle RPC result"))
    }

    async fn call(&self, address: &str, data: &str) -> Result<String, ServerError> {
        self.call_at(address, data, "latest").await
    }

    async fn call_at(&self, address: &str, data: &str, block: &str) -> Result<String, ServerError> {
        self.rpc("eth_call", json!([{ "to":address, "data":data }, block]))
            .await?
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| invalid("invalid oracle contract response"))
    }

    async fn assert_chain(&self) -> Result<(), ServerError> {
        let actual = self.rpc("eth_chainId", json!([])).await?;
        let chain = actual
            .as_str()
            .and_then(|s| s.strip_prefix("0x"))
            .and_then(|s| u64::from_str_radix(s, 16).ok());
        if chain != Some(self.chain_id) {
            return Err(invalid("native oracle RPC returned the wrong chain"));
        }
        Ok(())
    }

    /// A durable request reservation is not fresh authorization to issue access.
    /// Check the current vault state even on exact retries, without refreshing
    /// their proof-bound price/root. Call again after activation before exposing
    /// the provider secret, since withdrawal may occur during provider I/O.
    pub async fn assert_request_unspent(&self, nullifier: &Felt252) -> Result<(), ServerError> {
        let result = async {
            self.assert_chain().await?;
            let selector = Keccak256::digest(b"usedNullifiers(uint256)");
            let data = format!(
                "0x{}{}",
                hex::encode(&selector[..4]),
                hex::encode(nullifier.as_bytes())
            );
            let raw = self.call(&self.contract_address, &data).await?;
            if raw.len() != 66 {
                return Err(invalid("invalid vault nullifier response length"));
            }
            match decode_word(&raw, 0)? {
                0 => Ok(()),
                1 => Err(ServerError::NullifierUsed),
                _ => Err(invalid("invalid vault nullifier boolean")),
            }
        }
        .await;
        // A failed read is not evidence that the client's request is invalid.
        // Keep its reservation recoverable and tell the client it can retry.
        result.map_err(|error| match error {
            ServerError::NullifierUsed => error,
            _ => ServerError::Internal(format!("vault authorization check failed: {error}")),
        })
    }

    async fn assert_deployment(&self) -> Result<(), ServerError> {
        self.assert_chain().await?;
        let decimals = decode_word(
            &self.call(&self.config.feed_address, "0x313ce567").await?,
            0,
        )?;
        if decimals != self.config.decimals as u128 {
            return Err(invalid("native oracle decimals do not match deployment"));
        }
        // This getter is absent on legacy token vaults; do not permit a pricing-only relabel.
        let unit = decode_word(&self.call(&self.contract_address, "0x4d1352fd").await?, 0)?;
        if unit != 1_000_000_000 {
            return Err(invalid("vault is not the configured native ETH deployment"));
        }
        Ok(())
    }

    pub async fn quote(&self, now: u64) -> Result<NativeBillingQuote, ServerError> {
        self.assert_deployment().await?;
        // Finalized rounds cannot roll back during an ordinary head reorg.
        // This is also the price source for the SDK's displayed USD reference.
        let raw = self
            .call_at(&self.config.feed_address, "0xfeaf968c", "finalized")
            .await?;
        let quote = self.decode_quote(&raw)?;
        self.assert_fresh(&quote, now)?;
        Ok(quote)
    }

    pub async fn validate(&self, quote: &NativeBillingQuote, now: u64) -> Result<(), ServerError> {
        quote.validate_identity(&self.config, self.chain_id)?;
        self.assert_fresh(quote, now)?;
        self.assert_deployment().await?;
        let data = format!("0x9a6fc8f5{:064x}", decimal(&quote.round_id)?);
        if self.decode_quote(
            &self
                .call_at(&self.config.feed_address, &data, "finalized")
                .await?,
        )? != *quote
        {
            return Err(invalid(
                "native billing quote does not match its finalized on-chain oracle round",
            ));
        }
        let latest = self.decode_quote(
            &self
                .call_at(&self.config.feed_address, "0xfeaf968c", "finalized")
                .await?,
        )?;
        self.assert_fresh(&latest, now)?;
        if decimal(&latest.round_id)? > decimal(&quote.round_id)? {
            return Err(ServerError::NativeQuoteSuperseded);
        }
        if latest != *quote {
            return Err(invalid(
                "native billing quote is not the latest finalized oracle round",
            ));
        }
        Ok(())
    }

    fn decode_quote(&self, raw: &str) -> Result<NativeBillingQuote, ServerError> {
        if raw.len() != 2 + 5 * 64 {
            return Err(invalid("invalid oracle round data length"));
        }
        let round_id = decode_word(raw, 0)?;
        let started_at = decode_word(raw, 2)?;
        let updated_at_word = decode_word(raw, 3)?;
        let answered_in_round = decode_word(raw, 4)?;
        if started_at == 0 || started_at > updated_at_word || answered_in_round < round_id {
            return Err(invalid("native price oracle round is incomplete"));
        }
        let updated_at = u64::try_from(decode_word(raw, 3)?)
            .map_err(|_| invalid("oracle timestamp overflow"))?;
        let quote = NativeBillingQuote {
            asset: "native_eth".to_string(),
            units_per_eth: UNITS_PER_ETH as u64,
            chain_id: self.chain_id,
            feed_address: self.config.feed_address.to_lowercase(),
            round_id: decode_word(raw, 0)?.to_string(),
            answer: decode_word(raw, 1)?.to_string(),
            decimals: self.config.decimals,
            updated_at,
            expires_at: updated_at
                .checked_add(self.config.max_age_seconds)
                .ok_or_else(|| invalid("oracle expiry overflow"))?,
        };
        quote.validate_identity(&self.config, self.chain_id)?;
        Ok(quote)
    }

    pub(crate) fn assert_fresh(
        &self,
        quote: &NativeBillingQuote,
        now: u64,
    ) -> Result<(), ServerError> {
        if quote.updated_at > now {
            return Err(invalid("native ETH price is in the future"));
        }
        if quote.expires_at <= now {
            return Err(ServerError::NativeQuoteExpired);
        }
        Ok(())
    }
}

fn valid_address(value: &str) -> bool {
    value.len() == 42
        && value.starts_with("0x")
        && value[2..].bytes().all(|b| b.is_ascii_hexdigit())
        && value[2..].bytes().any(|b| b != b'0')
}
fn decode_word(raw: &str, index: usize) -> Result<u128, ServerError> {
    let word = raw
        .strip_prefix("0x")
        .and_then(|s| s.get(index * 64..(index + 1) * 64))
        .ok_or_else(|| invalid("malformed oracle ABI response"))?;
    if !word.bytes().all(|b| b.is_ascii_hexdigit()) || !word[..32].bytes().all(|b| b == b'0') {
        return Err(invalid("oracle value is negative or oversized"));
    }
    u128::from_str_radix(&word[32..], 16).map_err(|_| invalid("malformed oracle integer"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn quote() -> NativeBillingQuote {
        NativeBillingQuote {
            asset: "native_eth".into(),
            units_per_eth: 1_000_000_000,
            chain_id: 11155111,
            feed_address: "0x694aa1769357215de4fac081bf1f309adc325306".into(),
            round_id: "123".into(),
            answer: "250012345678".into(),
            decimals: 8,
            updated_at: 1_700_000_000,
            expires_at: 1_700_003_600,
        }
    }
    #[test]
    fn conservative_round_trip() {
        let q = quote();
        for units in [1u128, 99, 400_000, 2_000_000, 10_000_000] {
            let budget = q.limit_micro_usd(units).unwrap();
            assert!(q.charge_units(budget).unwrap() <= units);
            assert_eq!(q.charge_units(0).unwrap(), 0);
        }
    }
    #[test]
    fn one_micro_dollar_never_rounds_to_free() {
        assert_eq!(quote().charge_units(1).unwrap(), 1);
    }
    #[test]
    fn malformed_and_out_of_range_fail() {
        assert!(decimal("01").is_err());
        assert!(decimal("-1").is_err());
        assert!(quote().limit_micro_usd(MAX_SAFE_UNITS + 1).is_err());
        assert!(decode_word(&format!("0x{}", "f".repeat(64)), 0).is_err());
    }
    #[test]
    fn quote_identity_is_exact() {
        let mut q = quote();
        let c = NativeBillingConfig {
            rpc_url: "https://example.com".into(),
            feed_address: q.feed_address.clone(),
            decimals: 8,
            max_age_seconds: 3600,
        };
        assert!(q.validate_identity(&c, 11155111).is_ok());
        q.expires_at += 1;
        assert!(q.validate_identity(&c, 11155111).is_err());
    }

    #[tokio::test]
    async fn issuance_authorization_requires_an_explicit_unspent_vault_response() {
        use axum::{routing::post, Json, Router};
        use std::sync::{Arc, Mutex};

        let chain = Arc::new(Mutex::new(json!("0xaa36a7")));
        let response = Arc::new(Mutex::new(json!({
            "jsonrpc":"2.0", "id":1, "result":format!("0x{:064x}", 0)
        })));
        let calls = Arc::new(Mutex::new(Vec::<Value>::new()));
        let app = Router::new().route("/", post({
            let chain = chain.clone();
            let response = response.clone();
            let calls = calls.clone();
            move |Json(request): Json<Value>| {
                let chain = chain.clone();
                let response = response.clone();
                let calls = calls.clone();
                async move {
                    calls.lock().unwrap().push(request.clone());
                    if request["method"] == "eth_chainId" {
                        Json(json!({"jsonrpc":"2.0", "id":1, "result":chain.lock().unwrap().clone()}))
                    } else {
                        Json(response.lock().unwrap().clone())
                    }
                }
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let rpc_url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let vault = "0x0000000000000000000000000000000000000001";
        let oracle = NativeBillingOracle::new(
            NativeBillingConfig {
                rpc_url,
                feed_address: quote().feed_address,
                decimals: 8,
                max_age_seconds: 3600,
            },
            11155111,
            vault.into(),
        )
        .unwrap();
        let nullifier = Felt252::from_u64(123);
        oracle.assert_request_unspent(&nullifier).await.unwrap();
        {
            let requests = calls.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[0]["method"], "eth_chainId");
            assert_eq!(requests[1]["method"], "eth_call");
            // Independent known selector and ABI word, rather than repeating
            // the production encoder, guard the queried mapping and key.
            assert_eq!(
                requests[1]["params"],
                json!([
                    {"to":vault, "data":format!("0xaad24061{:064x}", 123)}, "latest"
                ])
            );
        }
        *response.lock().unwrap() =
            json!({"jsonrpc":"2.0", "id":1, "result":format!("0x{:064x}", 1)});
        assert!(matches!(
            oracle.assert_request_unspent(&nullifier).await,
            Err(ServerError::NullifierUsed)
        ));
        for malformed in [
            json!({"jsonrpc":"2.0", "id":1, "result":"0x"}),
            json!({"jsonrpc":"2.0", "id":1, "result":"0x0"}),
            json!({"jsonrpc":"2.0", "id":1, "result":format!("0x{:064x}", 2)}),
            json!({"jsonrpc":"2.0", "id":1, "result":format!("0x{}", "f".repeat(64))}),
            json!({"jsonrpc":"2.0", "id":1, "result":format!("0x{}", "0".repeat(128))}),
            json!({"jsonrpc":"2.0", "id":1, "result":false}),
            json!({"jsonrpc":"2.0", "id":1, "error":{"code":-32000}}),
            json!({"jsonrpc":"2.0", "id":2, "result":format!("0x{:064x}", 0)}),
        ] {
            *response.lock().unwrap() = malformed;
            let error = oracle.assert_request_unspent(&nullifier).await.unwrap_err();
            assert!(matches!(error, ServerError::Internal(_)));
            assert!(error.is_retriable());
        }
        *response.lock().unwrap() =
            json!({"jsonrpc":"2.0", "id":1, "result":format!("0x{:064x}", 0)});
        for wrong_chain in [
            json!("0x1"),
            json!("0xaa36a8"),
            json!(11155111),
            json!("junk"),
        ] {
            *chain.lock().unwrap() = wrong_chain;
            let before = calls.lock().unwrap().len();
            assert!(matches!(
                oracle.assert_request_unspent(&nullifier).await,
                Err(ServerError::Internal(_))
            ));
            assert_eq!(calls.lock().unwrap().len(), before + 1);
        }
        task.abort();
        let _ = task.await;
        assert!(matches!(
            oracle.assert_request_unspent(&nullifier).await,
            Err(ServerError::Internal(_))
        ));
    }

    #[tokio::test]
    async fn oracle_checks_chain_vault_round_and_freshness() {
        use axum::{routing::post, Json, Router};
        use std::sync::{
            atomic::{AtomicU64, Ordering},
            Arc,
        };
        let price = Arc::new(AtomicU64::new(250_012_345_678));
        let response_price = price.clone();
        let round_state = Arc::new(AtomicU64::new(0));
        let response_round_state = round_state.clone();
        let app = Router::new().route(
            "/",
            post(move |Json(request): Json<Value>| {
                let price = response_price.clone();
                let round_state = response_round_state.clone();
                async move {
                    let data = request["params"][0]["data"].as_str().unwrap_or("");
                    let result = if request["method"] == "eth_chainId" {
                        "0xaa36a7".to_string()
                    } else if data == "0x313ce567" {
                        format!("0x{:064x}", 8)
                    } else if data == "0x4d1352fd" {
                        format!("0x{:064x}", 1_000_000_000)
                    } else {
                        let state = round_state.load(Ordering::SeqCst);
                        let started = match state {
                            1 => 0,
                            2 => 1_700_000_001,
                            _ => 1_700_000_000,
                        };
                        let answered = if state == 3 { 122 } else { 123 };
                        format!(
                            "0x{:064x}{:064x}{:064x}{:064x}{:064x}",
                            123,
                            price.load(Ordering::SeqCst),
                            started,
                            1_700_000_000u64,
                            answered
                        )
                    };
                    Json(json!({"jsonrpc":"2.0","id":1,"result":result}))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let rpc_url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let config = NativeBillingConfig {
            rpc_url,
            feed_address: quote().feed_address,
            decimals: 8,
            max_age_seconds: 3600,
        };
        let oracle = NativeBillingOracle::new(
            config.clone(),
            11155111,
            "0x0000000000000000000000000000000000000001".into(),
        )
        .unwrap();
        for incomplete_state in 1..=3 {
            round_state.store(incomplete_state, Ordering::SeqCst);
            assert!(oracle.quote(1_700_000_001).await.is_err());
        }
        round_state.store(0, Ordering::SeqCst);
        let saved = oracle.quote(1_700_000_001).await.unwrap();
        oracle.validate(&saved, 1_700_000_002).await.unwrap();
        assert!(oracle.validate(&saved, 1_700_003_600).await.is_err());
        assert!(oracle.validate(&saved, 1_700_003_601).await.is_err());
        assert!(oracle.validate(&saved, 1_699_999_999).await.is_err());
        price.store(300_000_000_000, Ordering::SeqCst);
        assert!(oracle.validate(&saved, 1_700_000_003).await.is_err());
        let wrong_chain = NativeBillingOracle::new(
            config,
            1,
            "0x0000000000000000000000000000000000000001".into(),
        )
        .unwrap();
        assert!(wrong_chain.quote(1_700_000_001).await.is_err());
        task.abort();
    }
}
