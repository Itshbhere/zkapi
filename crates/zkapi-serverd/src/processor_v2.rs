//! zkAPI v2 request processor.

use std::sync::{Arc, RwLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use base64::Engine;
use sha3::{Digest, Keccak256};
use zkapi_core::v2 as core;
use zkapi_proof::compact::{random_field, random_scalar, server_update, RequestVerifier};
use zkapi_types::wire::{
    ApiRequestV2, ClearanceRequest, ClearanceResponseV2, CurvePointWire,
    OpenRouterLeaseAuthorization, OpenRouterLeaseResponse, OpenRouterLeaseStatusResponse,
    RecoveryResponseV2, RequestResponseV2,
};
use zkapi_types::{
    canonical_payload_hash, canonical_request_context, canonical_response_hash, Felt252,
    NullifierStatus,
};

use crate::config::{OpenRouterLeaseSourceConfig, ServerConfig};
use crate::dashboard::{
    charge_usd, decode_request_view, redact_secrets, DashboardEvent, DashboardHub,
};
use crate::error::ServerError;
use crate::native_billing::{NativeBillingOracle, NativeBillingQuote, MAX_SAFE_UNITS};
use crate::nullifier_store::{api_request_binding, NullifierStore, TranscriptRecord};
use crate::oa_org::{IssuedOpenRouterLease, OaOrgProvisioner, OaOrgUsage, OaOrgUsageExpectation};
use crate::openrouter::OpenRouterProvisioner;
use crate::pricing;
use crate::provider::{ApiProvider, ProviderResponse, UsageInfo};
use crate::signer::ServerSigner;

const MAX_REQUEST_AGE_SECONDS: u64 = 300;
const MAX_FUTURE_SKEW_SECONDS: u64 = 30;
// OA's signed validity is an upstream upper bound, not a guarantee that the
// child key remains usable until the final second. Stop advertising it as the
// usable lease end and leave room for clock skew and in-flight requests.
const OA_LEASE_EXPIRY_SAFETY_SECONDS: u64 = 30;

#[derive(Clone, Copy)]
enum ReservationKind {
    Proxy,
    OpenRouterLease,
}

impl ReservationKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Proxy => "proxy",
            Self::OpenRouterLease => "openrouter_lease",
        }
    }
}

pub struct RequestProcessor {
    config: ServerConfig,
    store: Arc<NullifierStore>,
    signer: Arc<ServerSigner>,
    verifier: RequestVerifier,
    provider: Arc<dyn ApiProvider>,
    current_root: Arc<RwLock<Felt252>>,
    dashboard: Option<Arc<DashboardHub>>,
    openrouter: Option<Arc<OpenRouterProvisioner>>,
    oa_org: Option<Arc<OaOrgProvisioner>>,
    native_oracle: Option<NativeBillingOracle>,
    lease_issue_lock: tokio::sync::Mutex<()>,
    lease_settlement_lock: tokio::sync::Mutex<()>,
}

impl RequestProcessor {
    pub fn try_new(
        config: ServerConfig,
        store: Arc<NullifierStore>,
        signer: Arc<ServerSigner>,
        provider: Arc<dyn ApiProvider>,
        current_root: Felt252,
    ) -> anyhow::Result<Self> {
        if let Some(lease) = config.openrouter_leases.as_ref() {
            anyhow::ensure!(
                lease.ttl_seconds > 0,
                "OpenRouter lease TTL must be positive"
            );
            match &lease.source {
                OpenRouterLeaseSourceConfig::OpenRouter { management_key, .. } => {
                    anyhow::ensure!(
                        !management_key.is_empty(),
                        "OpenRouter management key cannot be empty"
                    );
                }
                OpenRouterLeaseSourceConfig::OaOrg {
                    org_base_url,
                    shared_secret,
                } => {
                    anyhow::ensure!(!org_base_url.is_empty(), "OA org URL cannot be empty");
                    anyhow::ensure!(
                        !shared_secret.is_empty(),
                        "OA org shared secret cannot be empty"
                    );
                    anyhow::ensure!(
                        lease.ttl_seconds.is_multiple_of(60),
                        "OA org lease TTL must be a whole number of minutes"
                    );
                }
            }
        }
        if config.native_billing.is_some() {
            anyhow::ensure!(
                config.request_charge_cap > 0 && config.request_charge_cap <= MAX_SAFE_UNITS,
                "native request cap must fit browser-safe gwei units"
            );
            anyhow::ensure!(
                config.openrouter_leases.is_some() && !config.policy_enabled,
                "native ETH requires prompt-private leases without proxy policy"
            );
        }
        let native_oracle = config
            .native_billing
            .clone()
            .map(|native| {
                let address = config.contract_address.to_hex();
                let body = address.strip_prefix("0x").unwrap_or(&address);
                NativeBillingOracle::new(native, config.chain_id, format!("0x{:0>40}", body))
            })
            .transpose()?;
        let verifier = RequestVerifier::load(&config.proof_setup_dir)?;
        let openrouter = config
            .openrouter_leases
            .as_ref()
            .and_then(|lease| match &lease.source {
                OpenRouterLeaseSourceConfig::OpenRouter {
                    management_key,
                    api_base,
                } => Some(
                    OpenRouterProvisioner::new(management_key.clone(), api_base.clone())
                        .map(Arc::new),
                ),
                OpenRouterLeaseSourceConfig::OaOrg { .. } => None,
            })
            .transpose()?;
        let oa_org = config
            .openrouter_leases
            .as_ref()
            .and_then(|lease| match &lease.source {
                OpenRouterLeaseSourceConfig::OaOrg {
                    org_base_url,
                    shared_secret,
                } => Some(
                    OaOrgProvisioner::new(org_base_url.clone(), shared_secret.clone())
                        .map(Arc::new),
                ),
                OpenRouterLeaseSourceConfig::OpenRouter { .. } => None,
            })
            .transpose()?;
        Ok(Self {
            native_oracle,
            config,
            store,
            signer,
            verifier,
            provider,
            current_root: Arc::new(RwLock::new(current_root)),
            dashboard: None,
            openrouter,
            oa_org,
            lease_issue_lock: tokio::sync::Mutex::new(()),
            lease_settlement_lock: tokio::sync::Mutex::new(()),
        })
    }

    pub fn with_dashboard(mut self, dashboard: Arc<DashboardHub>) -> Self {
        self.dashboard = Some(dashboard);
        self
    }

    pub fn dashboard(&self) -> Option<&Arc<DashboardHub>> {
        self.dashboard.as_ref()
    }

    pub fn update_root(&self, new_root: Felt252) {
        if let Ok(mut root) = self.current_root.write() {
            *root = new_root;
        }
    }

    pub fn current_root(&self) -> Felt252 {
        self.current_root
            .read()
            .map(|value| *value)
            .unwrap_or(Felt252::ZERO)
    }

    pub fn state_signing_key(&self) -> CurvePointWire {
        self.signer.state_public_key()
    }

    pub fn clearance_signing_key(&self) -> CurvePointWire {
        self.signer.clearance_public_key()
    }

    pub async fn native_billing_quote(&self) -> Result<NativeBillingQuote, ServerError> {
        self.native_oracle
            .as_ref()
            .ok_or_else(|| ServerError::InvalidRequest("native ETH billing is not enabled".into()))?
            .quote(current_timestamp())
            .await
    }

    fn lease_authorization(
        &self,
        request: &ApiRequestV2,
    ) -> Result<(OpenRouterLeaseAuthorization, Option<NativeBillingQuote>), ServerError> {
        let mut payload: serde_json::Value = serde_json::from_str(&request.payload)
            .map_err(|_| ServerError::InvalidRequest("invalid lease authorization".into()))?;
        let object = payload.as_object_mut().ok_or_else(|| {
            ServerError::InvalidRequest("invalid lease authorization object".into())
        })?;
        let quote = object
            .remove("billing_quote")
            .map(serde_json::from_value::<NativeBillingQuote>)
            .transpose()
            .map_err(|_| ServerError::InvalidRequest("invalid native billing quote".into()))?;
        match (&self.config.native_billing, &quote) {
            (Some(config), Some(quote)) => quote.validate_identity(config, self.config.chain_id)?,
            (None, None) => {}
            _ => {
                return Err(ServerError::InvalidRequest(
                    "lease quote does not match billing asset".into(),
                ))
            }
        }
        let authorization = serde_json::from_value(payload).map_err(|_| {
            ServerError::InvalidRequest("invalid prompt-free lease authorization".into())
        })?;
        Ok((authorization, quote))
    }

    fn lease_limit_micro_usd(&self, request: &ApiRequestV2) -> Result<u128, ServerError> {
        match self.lease_authorization(request)?.1 {
            Some(quote) => quote.limit_micro_usd(request.public_inputs.solvency_bound),
            None => Ok(request.public_inputs.solvency_bound),
        }
    }

    fn settlement_payload(
        &self,
        request: &ApiRequestV2,
        mut payload: serde_json::Value,
    ) -> Result<String, ServerError> {
        if let Some(quote) = self.lease_authorization(request)?.1 {
            payload
                .as_object_mut()
                .ok_or_else(|| ServerError::Internal("settlement payload is not an object".into()))?
                .insert(
                    "billing_quote".into(),
                    serde_json::to_value(quote).map_err(|_| {
                        ServerError::Internal("could not encode native billing quote".into())
                    })?,
                );
        }
        Ok(payload.to_string())
    }

    fn lease_charge_units(
        &self,
        request: &ApiRequestV2,
        micro_usd: u128,
    ) -> Result<u128, ServerError> {
        match self.lease_authorization(request)?.1 {
            Some(quote) => quote.charge_units(micro_usd),
            None => Ok(micro_usd),
        }
    }

    pub async fn process_request(
        &self,
        request: &ApiRequestV2,
    ) -> Result<RequestResponseV2, ServerError> {
        if self.native_oracle.is_some() {
            return Err(ServerError::InvalidRequest(
                "native ETH supports prompt-private leases only; proxy billing is disabled".into(),
            ));
        }
        let started = Instant::now();
        if let Some(response) = self.validate_and_reserve(request, ReservationKind::Proxy)? {
            return Ok(response);
        }
        let upstream_started = Instant::now();
        let provider_response = self
            .provider
            .execute(
                &request.client_request_id,
                &request.payload,
                &request.payload_hash,
            )
            .await?;
        self.finalize_request(
            request,
            provider_response,
            upstream_started.elapsed().as_millis() as u64,
            started.elapsed().as_millis() as u64,
        )
    }

    fn native_oa_request_id(request: &ApiRequestV2) -> Result<String, ServerError> {
        let binding = api_request_binding(request)?;
        let mut hash = Keccak256::new();
        hash.update(b"zkapi:oa-org-request:v1\0");
        hash.update(binding.as_bytes());
        Ok(format!("zkapi-v2-{}", hex::encode(hash.finalize())))
    }

    fn persisted_oa_request_id(
        &self,
        lease: &crate::nullifier_store::OpenRouterLeaseRecord,
    ) -> Result<String, ServerError> {
        if self.config.native_billing.is_none() {
            return Ok(lease
                .oa_client_request_id
                .clone()
                .unwrap_or_else(|| lease.client_request_id.clone()));
        }
        let expected = Self::native_oa_request_id(&lease.api_request)?;
        if lease.oa_client_request_id.as_deref() != Some(expected.as_str()) {
            return Err(ServerError::Internal(
                "native OA lease is missing its bound upstream request ID".into(),
            ));
        }
        Ok(expected)
    }

    /// Reserve one prompt-free zkAPI request and mint its bounded OpenRouter
    /// runtime key. Ordinary proxied requests continue to use `/v2/requests`.
    pub async fn issue_openrouter_lease(
        &self,
        request: &ApiRequestV2,
    ) -> Result<IssuedOpenRouterLease, ServerError> {
        let (authorization, billing_quote) = self.lease_authorization(request)?;
        if authorization != OpenRouterLeaseAuthorization::default() {
            return Err(ServerError::InvalidRequest(
                "unsupported OpenRouter lease authorization".to_string(),
            ));
        }
        let lease_config = self.config.openrouter_leases.as_ref().ok_or_else(|| {
            ServerError::InvalidRequest(
                "prompt-private OpenRouter leases are not enabled on this server".to_string(),
            )
        })?;
        if self.config.policy_enabled {
            return Err(ServerError::InvalidRequest(
                "prompt-private leases cannot enforce server-side prompt policy".to_string(),
            ));
        }
        // Validate native integer bounds before reserving a nullifier. An
        // unrepresentable budget must never strand otherwise unused state.
        let limit_micro_usd = self.lease_limit_micro_usd(request)?;
        let _issue_guard = self.lease_issue_lock.lock().await;
        let oa_request_id = if self.config.native_billing.is_some()
            && matches!(
                lease_config.source,
                OpenRouterLeaseSourceConfig::OaOrg { .. }
            ) {
            Some(Self::native_oa_request_id(request)?)
        } else {
            None
        };

        // A persisted request already froze its quote before external issuance.
        // Matching retries remain valid after the oracle round ages out.
        let existing_lease = self
            .store
            .lookup_openrouter_lease(&request.client_request_id);
        let existing_reservation = self
            .store
            .lookup_by_nullifier(&request.public_inputs.request_nullifier);
        if self.native_oracle.is_some() {
            if let Some(existing) = existing_lease.as_ref() {
                if api_request_binding(&existing.api_request)? != api_request_binding(request)? {
                    return Err(ServerError::Replay);
                }
            }
        }
        if existing_reservation.is_none() {
            if let (Some(oracle), Some(quote)) = (&self.native_oracle, &billing_quote) {
                oracle.validate(quote, current_timestamp()).await?;
                // Oracle reads may span the expiration boundary. No await may
                // separate this check from proof validation/reservation below.
                oracle.assert_fresh(quote, current_timestamp())?;
            }
        }
        if self
            .validate_and_reserve(request, ReservationKind::OpenRouterLease)?
            .is_some()
        {
            return Err(ServerError::Replay);
        }
        let key_name = format!("zkapi-{}", request.client_request_id);
        // The verified proof may expose a coarse solvency tier above the
        // deployment's minimum request cap. Bind that exact tier to the child
        // key's cumulative USD budget for this chat.
        let mut spending_limit_usd = pricing::credits_to_usd(limit_micro_usd);
        if !spending_limit_usd.is_finite() || spending_limit_usd <= 0.0 {
            return Err(ServerError::InvalidRequest(
                "lease spending limit must be positive".to_string(),
            ));
        }
        let mut resume_provisioning = false;
        let mut issued_at = current_timestamp();
        if let Some(existing) = self
            .store
            .lookup_openrouter_lease(&request.client_request_id)
        {
            let persisted_binding = api_request_binding(&existing.api_request)?;
            if existing.request_nullifier != request.public_inputs.request_nullifier
                || persisted_binding != api_request_binding(request)?
            {
                return Err(ServerError::Internal(
                    "pending lease request does not match its nullifier reservation".to_string(),
                ));
            }
            if existing.key_source != lease_config.source.label() {
                return Err(ServerError::Internal(format!(
                    "pending lease source {} does not match configured source {}",
                    existing.key_source,
                    lease_config.source.label()
                )));
            }
            if existing.status != "provisioning" || existing.key_hash.is_some() {
                return Err(ServerError::LeasePending);
            }
            match &lease_config.source {
                OpenRouterLeaseSourceConfig::OpenRouter { .. } => {
                    let provisioner = self.openrouter.as_ref().ok_or_else(|| {
                        ServerError::Internal(
                            "OpenRouter lease provider is unavailable".to_string(),
                        )
                    })?;
                    provisioner.delete_keys_named(&key_name).await?;
                    self.store
                        .remove_failed_openrouter_lease(&request.client_request_id)?;
                }
                OpenRouterLeaseSourceConfig::OaOrg { .. } => {
                    // OA station issuance is replay-safe. Retain the durable
                    // reservation and ask for the same one-show key again. The
                    // persisted limit, not any retry input, is authoritative.
                    let persisted_limit =
                        pricing::credits_to_usd(self.lease_limit_micro_usd(&existing.api_request)?);
                    if existing.spending_limit_usd.to_bits() != persisted_limit.to_bits()
                        || !persisted_limit.is_finite()
                        || persisted_limit <= 0.0
                    {
                        return Err(ServerError::Internal(
                            "pending lease spending limit does not match its bound request"
                                .to_string(),
                        ));
                    }
                    self.persisted_oa_request_id(&existing)?;
                    spending_limit_usd = existing.spending_limit_usd;
                    resume_provisioning = true;
                    issued_at = existing.issued_at;
                }
            }
        }
        let requested_expires_at = current_timestamp().saturating_add(lease_config.ttl_seconds);
        if !resume_provisioning {
            self.store.create_openrouter_lease_with_oa_id(
                request,
                lease_config.source.label(),
                issued_at,
                requested_expires_at,
                requested_expires_at.saturating_add(lease_config.settlement_grace_seconds),
                spending_limit_usd,
                oa_request_id.as_deref(),
            )?;
        }
        let (api_key, key_hash, openrouter_api_base, expires_at, verification) = match &lease_config
            .source
        {
            OpenRouterLeaseSourceConfig::OpenRouter { .. } => {
                let provisioner = self.openrouter.as_ref().ok_or_else(|| {
                    ServerError::Internal("OpenRouter lease provider is unavailable".to_string())
                })?;
                match provisioner
                    .create_key(&key_name, spending_limit_usd, requested_expires_at)
                    .await
                {
                    Ok(created) => (
                        created.key,
                        created.hash,
                        provisioner.inference_base(),
                        requested_expires_at,
                        None,
                    ),
                    Err(error) => {
                        let _ = self
                            .store
                            .remove_failed_openrouter_lease(&request.client_request_id);
                        return Err(error);
                    }
                }
            }
            OpenRouterLeaseSourceConfig::OaOrg { .. } => {
                let provisioner = self.oa_org.as_ref().ok_or_else(|| {
                    ServerError::Internal("OA org lease provider is unavailable".to_string())
                })?;
                let created = provisioner
                    .create_key(
                        oa_request_id
                            .as_deref()
                            .unwrap_or(&request.client_request_id),
                        spending_limit_usd,
                        limit_micro_usd,
                        lease_config.ttl_seconds,
                    )
                    .await?;
                let usable_expires_at = created
                    .expires_at
                    .saturating_sub(OA_LEASE_EXPIRY_SAFETY_SECONDS);
                if usable_expires_at <= current_timestamp() {
                    return Err(ServerError::Internal(
                        "OA org key has no safe usable lifetime".to_string(),
                    ));
                }
                (
                    created.key,
                    created.hash,
                    created.openrouter_api_base,
                    usable_expires_at,
                    Some(created.verification),
                )
            }
        };
        let settle_after = expires_at.saturating_add(lease_config.settlement_grace_seconds);
        self.store.update_openrouter_lease_timing(
            &request.client_request_id,
            expires_at,
            settle_after,
        )?;
        if let Err(error) = self
            .store
            .activate_openrouter_lease(&request.client_request_id, &key_hash)
        {
            if let (Some(provisioner), OpenRouterLeaseSourceConfig::OpenRouter { .. }) =
                (&self.openrouter, &lease_config.source)
            {
                let _ = provisioner.delete_key(&key_hash).await;
            }
            return Err(error);
        }
        Ok(IssuedOpenRouterLease {
            lease: OpenRouterLeaseResponse {
                status: "active".to_string(),
                client_request_id: request.client_request_id.clone(),
                api_key,
                openrouter_api_base,
                issued_at,
                expires_at,
                valid_for_seconds: expires_at.saturating_sub(issued_at),
                settle_after,
                spending_limit_usd,
            },
            key_source: lease_config.source.label().to_string(),
            verification,
            billing_quote,
        })
    }

    /// Read-only expiration handshake. It serializes behind in-flight issuance
    /// without provisioning a key or mutating the wallet/nullifier database.
    pub async fn expire_unaccepted_native_lease(
        &self,
        client_request_id: &str,
        request: &ApiRequestV2,
    ) -> Result<serde_json::Value, ServerError> {
        let (authorization, quote) = self.lease_authorization(request)?;
        let quote = quote.ok_or_else(|| {
            ServerError::InvalidRequest("native ETH expiry check requires a bound quote".into())
        })?;
        if authorization != OpenRouterLeaseAuthorization::default()
            || request.client_request_id != client_request_id
            || canonical_payload_hash(request.payload.as_bytes()) != request.payload_hash
        {
            return Err(ServerError::InvalidRequest(
                "native expiry check does not match the exact request".into(),
            ));
        }
        if request.public_inputs.protocol_version != self.config.protocol_version
            || request.public_inputs.chain_id != self.config.chain_id
            || request.public_inputs.contract_address != self.config.contract_address
        {
            return Err(ServerError::ProtocolMismatch(
                "native expiry check deployment mismatch".into(),
            ));
        }
        let _guard = self.lease_issue_lock.lock().await;
        if self
            .store
            .lookup_by_nullifier(&request.public_inputs.request_nullifier)
            .is_some()
            || self
                .store
                .lookup_openrouter_lease(client_request_id)
                .is_some()
            || self.store.lookup_by_client_id(client_request_id).is_some()
        {
            return Err(ServerError::LeasePending);
        }
        let status = if quote.expires_at <= current_timestamp() {
            "expired_unaccepted"
        } else {
            let oracle = self.native_oracle.as_ref().ok_or_else(|| {
                ServerError::InvalidRequest("native expiry check requires native oracle".into())
            })?;
            match oracle.validate(&quote, current_timestamp()).await {
                Err(ServerError::NativeQuoteSuperseded) => "superseded_unaccepted",
                Err(ServerError::NativeQuoteExpired) if quote.expires_at <= current_timestamp() => {
                    "expired_unaccepted"
                }
                Err(error) => return Err(error),
                Ok(()) if quote.expires_at <= current_timestamp() => "expired_unaccepted",
                Ok(()) => return Err(ServerError::LeasePending),
            }
        };
        Ok(serde_json::json!({
            "status":status, "client_request_id":client_request_id,
            "request_nullifier":request.public_inputs.request_nullifier,
            "payload_hash":request.payload_hash, "server_time_ms":current_timestamp().saturating_mul(1000),
        }))
    }

    /// Settle every expired lease from OpenRouter's authoritative aggregate
    /// usage. Failures are retained and retried by the next background scan.
    pub async fn settle_due_openrouter_leases(&self) {
        let _settlement_guard = self.lease_settlement_lock.lock().await;
        for lease in self.store.due_openrouter_leases(current_timestamp()) {
            let result = match lease.key_source.as_str() {
                "openrouter" => match &self.openrouter {
                    Some(provisioner) => self.settle_openrouter_lease(&lease, provisioner).await,
                    None => Err(ServerError::Internal(
                        "direct OpenRouter settlement credential is unavailable".to_string(),
                    )),
                },
                "oa_org" => self.settle_oa_org_lease(&lease).await,
                source => Err(ServerError::Internal(format!(
                    "unsupported lease key source {source}"
                ))),
            };
            if let Err(error) = result {
                tracing::warn!(
                    client_request_id = %lease.client_request_id,
                    error = %error,
                    "OpenRouter lease settlement will be retried"
                );
                let _ = self
                    .store
                    .record_openrouter_lease_error(&lease.client_request_id, &error.to_string());
            }
        }
    }

    /// Retire a key that the upstream rejected before its advertised lease
    /// end. Requiring the original prompt-free proof prevents lease IDs alone
    /// from acting as unauthenticated denial-of-service capabilities.
    pub async fn retire_openrouter_lease(
        &self,
        client_request_id: &str,
        request: &ApiRequestV2,
    ) -> Result<OpenRouterLeaseStatusResponse, ServerError> {
        let _settlement_guard = self.lease_settlement_lock.lock().await;
        let lease = self
            .store
            .lookup_openrouter_lease(client_request_id)
            .ok_or_else(|| ServerError::InvalidRequest("unknown OpenRouter lease".to_string()))?;
        let request_matches = serde_json::to_value(request)
            .and_then(|request| {
                serde_json::to_value(&lease.api_request).map(|issued| request == issued)
            })
            .map_err(|error| {
                ServerError::Internal(format!("failed to compare lease retirement proof: {error}"))
            })?;
        if request.client_request_id != client_request_id || !request_matches {
            return Err(ServerError::InvalidRequest(
                "lease retirement proof does not match the issued lease".to_string(),
            ));
        }
        match lease.status.as_str() {
            "finalized" => {}
            "active" => match lease.key_source.as_str() {
                "openrouter" => {
                    let provisioner = self.openrouter.as_ref().ok_or_else(|| {
                        ServerError::Internal(
                            "direct OpenRouter settlement credential is unavailable".to_string(),
                        )
                    })?;
                    self.settle_openrouter_lease(&lease, provisioner).await?;
                }
                "oa_org" => self.settle_oa_org_lease(&lease).await?,
                source => {
                    return Err(ServerError::Internal(format!(
                        "unsupported lease key source {source}"
                    )))
                }
            },
            _ => return Err(ServerError::LeasePending),
        }
        self.openrouter_lease_status(client_request_id)
            .ok_or_else(|| ServerError::Internal("retired lease disappeared".to_string()))
    }

    /// Ask the OA org for the station-signed final child-key usage. Pending or
    /// unavailable receipts are retried; the reserved cap is never substituted.
    async fn settle_oa_org_lease(
        &self,
        lease: &crate::nullifier_store::OpenRouterLeaseRecord,
    ) -> Result<(), ServerError> {
        if let Some(record) = self
            .store
            .lookup_by_nullifier(&lease.request_nullifier)
            .filter(|record| record.status == NullifierStatus::Finalized)
        {
            let usage_credits = record
                .response_payload
                .as_deref()
                .and_then(|payload| serde_json::from_str::<serde_json::Value>(payload).ok())
                .and_then(|payload| payload["usage_credits"].as_u64())
                .map(u128::from)
                .unwrap_or_default();
            self.store.finalize_openrouter_lease(
                &lease.client_request_id,
                pricing::credits_to_usd(usage_credits),
                record.charge_applied.unwrap_or_default(),
            )?;
            return Ok(());
        }
        let key_hash = lease.key_hash.as_deref().ok_or_else(|| {
            ServerError::Internal("active OA org lease has no key hash".to_string())
        })?;
        let provisioner = self.oa_org.as_ref().ok_or_else(|| {
            ServerError::Internal("OA org lease provider is unavailable".to_string())
        })?;
        let lease_config = self.config.openrouter_leases.as_ref().ok_or_else(|| {
            ServerError::Internal("OA org lease configuration is unavailable".to_string())
        })?;
        if lease_config.ttl_seconds == 0 || !lease_config.ttl_seconds.is_multiple_of(60) {
            return Err(ServerError::Internal(
                "OA org lease duration is not a positive whole number of minutes".to_string(),
            ));
        }
        let duration_minutes = lease_config.ttl_seconds / 60;
        let expected_limit_credits = self.lease_limit_micro_usd(&lease.api_request)?;
        let oa_request_id = self.persisted_oa_request_id(lease)?;
        let receipt = provisioner
            .get_key_usage(
                &oa_request_id,
                key_hash,
                OaOrgUsageExpectation {
                    credit_limit_usd: lease.spending_limit_usd,
                    duration_minutes,
                    limit_credits: expected_limit_credits,
                    minimum_expires_at: lease.expires_at,
                    maximum_expires_at: lease
                        .expires_at
                        .saturating_add(OA_LEASE_EXPIRY_SAFETY_SECONDS),
                },
            )
            .await?;
        let receipt = match receipt {
            OaOrgUsage::Finalized(receipt) => receipt,
            OaOrgUsage::Pending {
                retry_after_seconds,
            } => {
                return Err(ServerError::LeaseSettlementPending {
                    retry_after_seconds,
                })
            }
        };
        if receipt.closed_at < lease.issued_at {
            return Err(ServerError::Internal(
                "OA org usage receipt predates the issued lease".to_string(),
            ));
        }
        let charge = self.lease_charge_units(&lease.api_request, receipt.usage_credits)?;
        let usage_usd = pricing::credits_to_usd(receipt.usage_credits);
        let payload = self.settlement_payload(
            &lease.api_request,
            serde_json::json!({
                "type": "oa_org_ephemeral_lease_settlement",
                "issued_at": lease.issued_at,
                "expires_at": lease.expires_at,
                "usage_credits": receipt.usage_credits,
                "usage_usd": usage_usd,
                "usage_receipt_expires_at": receipt.expires_at,
                "usage_receipt_closed_at": receipt.closed_at,
                "usage_finalized_at": receipt.finalized_at,
                "station_id": receipt.station_id,
                "station_signature": receipt.station_signature,
                "org_signature": receipt.org_signature,
            }),
        )?;
        let provider_response = ProviderResponse {
            status_code: 200,
            payload,
            charge_applied: charge,
            policy_reason_code: None,
            policy_evidence_hash: None,
            usage: Some(UsageInfo {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
                cost_usd: usage_usd,
                cost_source: "oa_org_signed_usage_receipt".to_string(),
            }),
            upstream_model: None,
            billing_label: "direct:oa-org-ephemeral".to_string(),
        };
        self.finalize_request(&lease.api_request, provider_response, 0, 0)?;
        self.store
            .finalize_openrouter_lease(&lease.client_request_id, usage_usd, charge)
    }

    async fn settle_openrouter_lease(
        &self,
        lease: &crate::nullifier_store::OpenRouterLeaseRecord,
        provisioner: &OpenRouterProvisioner,
    ) -> Result<(), ServerError> {
        if let Some(record) = self
            .store
            .lookup_by_nullifier(&lease.request_nullifier)
            .filter(|record| record.status == NullifierStatus::Finalized)
        {
            let usage_usd = record
                .response_payload
                .as_deref()
                .and_then(|payload| serde_json::from_str::<serde_json::Value>(payload).ok())
                .and_then(|payload| payload["usage_usd"].as_f64())
                .unwrap_or_default();
            self.store.finalize_openrouter_lease(
                &lease.client_request_id,
                usage_usd,
                record.charge_applied.unwrap_or_default(),
            )?;
            return Ok(());
        }
        let key_hash = lease.key_hash.as_deref().ok_or_else(|| {
            ServerError::Internal("active OpenRouter lease has no key hash".to_string())
        })?;
        let usage = provisioner.get_key_usage(key_hash).await?;
        if !usage.usage_usd.is_finite() || usage.usage_usd < 0.0 {
            return Err(ServerError::InvalidRequest(
                "invalid upstream USD usage".into(),
            ));
        }
        let raw_charge =
            self.lease_charge_units(&lease.api_request, pricing::usd_to_credits(usage.usage_usd))?;
        let lease_charge_cap = lease.api_request.public_inputs.solvency_bound;
        let charge = raw_charge.min(lease_charge_cap);
        if charge != raw_charge {
            tracing::warn!(
                client_request_id = %lease.client_request_id,
                raw_charge,
                charge,
                "OpenRouter usage exceeded the proof-bound lease limit; clamped charge"
            );
        }
        let payload = self.settlement_payload(
            &lease.api_request,
            serde_json::json!({
                "type": "openrouter_ephemeral_lease_settlement",
                "issued_at": lease.issued_at,
                "expires_at": lease.expires_at,
                "usage_usd": usage.usage_usd,
            }),
        )?;
        let provider_response = ProviderResponse {
            status_code: 200,
            payload,
            charge_applied: charge,
            policy_reason_code: None,
            policy_evidence_hash: None,
            usage: Some(UsageInfo {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
                cost_usd: usage.usage_usd,
                cost_source: "openrouter_key_aggregate".to_string(),
            }),
            upstream_model: None,
            billing_label: "direct:openrouter-ephemeral".to_string(),
        };
        self.finalize_request(&lease.api_request, provider_response, 0, 0)?;
        self.store
            .finalize_openrouter_lease(&lease.client_request_id, usage.usage_usd, charge)?;
        if let Err(error) = provisioner.delete_key(key_hash).await {
            tracing::warn!(
                client_request_id = %lease.client_request_id,
                error = %error,
                "expired OpenRouter key could not be deleted"
            );
        }
        Ok(())
    }

    /// Validate proof/deployment bindings and reserve the request nullifier.
    /// Returns a prior finalized response for an idempotent replay.
    fn validate_and_reserve(
        &self,
        request: &ApiRequestV2,
        reservation_kind: ReservationKind,
    ) -> Result<Option<RequestResponseV2>, ServerError> {
        let public = &request.public_inputs;
        let payload_hash = canonical_payload_hash(request.payload.as_bytes());
        if payload_hash != request.payload_hash {
            return Err(ServerError::InvalidRequest(
                "payload_hash does not match payload bytes".to_string(),
            ));
        }
        if public.protocol_version != self.config.protocol_version
            || public.chain_id != self.config.chain_id
            || public.contract_address != self.config.contract_address
        {
            return Err(ServerError::ProtocolMismatch(
                "version, chain, or contract mismatch".to_string(),
            ));
        }
        let request_binding = api_request_binding(request)?;
        // A byte-identical request already reserved by this endpoint passed
        // all checks on its first attempt. Resume it before root/freshness
        // checks so a transport retry remains possible after those values move.
        // A different endpoint kind can never claim the reservation.
        if let Some(existing) = self.store.lookup_by_nullifier(&public.request_nullifier) {
            let same_request = existing.client_request_id.as_deref()
                == Some(&request.client_request_id)
                && existing.payload_hash == Some(request.payload_hash)
                && existing.reservation_kind == reservation_kind.as_str()
                && existing.api_request_binding.as_deref() == Some(request_binding.as_str());
            if !same_request {
                return Err(ServerError::Replay);
            }
            return match existing.status {
                NullifierStatus::Finalized => {
                    response_from_record(&existing, &request.client_request_id).map(Some)
                }
                NullifierStatus::Reserved => Ok(None),
                NullifierStatus::ClearanceReserved => Err(ServerError::Replay),
            };
        }
        let root = self.current_root();
        if public.active_root != root {
            return Err(ServerError::StaleRoot {
                latest_root: root.to_hex(),
            });
        }
        let state_key = self.state_signing_key();
        if public.state_signing_key_x != state_key.x || public.state_signing_key_y != state_key.y {
            return Err(ServerError::InvalidRequest(
                "state signing key does not match this deployment".to_string(),
            ));
        }
        let now = current_timestamp();
        if public.request_time.saturating_add(MAX_REQUEST_AGE_SECONDS) < now
            || public.request_time > now.saturating_add(MAX_FUTURE_SKEW_SECONDS)
        {
            return Err(ServerError::InvalidRequest(
                "request_time is outside the accepted freshness window".to_string(),
            ));
        }
        let required_solvency = if self.config.policy_enabled {
            self.config.policy_charge_cap
        } else {
            self.config.request_charge_cap
        };
        if public.solvency_bound < required_solvency {
            return Err(ServerError::InvalidRequest(format!(
                "solvency_bound {} is below required {}",
                public.solvency_bound, required_solvency
            )));
        }
        let context = canonical_request_context(&request.client_request_id, &payload_hash);
        if core::authorization_tag(&public.request_nullifier, &context) != public.authorization_tag
        {
            return Err(ServerError::InvalidProof(
                "proof authorization tag does not bind this request id and payload".to_string(),
            ));
        }
        if !self
            .verifier
            .verify(public, &request.proof)
            .map_err(|error| ServerError::InvalidProof(error.to_string()))?
        {
            return Err(ServerError::InvalidProof(
                "Groth16 verification returned false".to_string(),
            ));
        }

        // Proof verification is synchronous but can be slow. Recheck at the
        // actual reservation boundary, while issue_openrouter_lease retains
        // its serialization lock. A matching existing reservation returned
        // above and keeps its original quote even after expiration.
        if matches!(reservation_kind, ReservationKind::OpenRouterLease) {
            if let (Some(oracle), Some(quote)) =
                (&self.native_oracle, self.lease_authorization(request)?.1)
            {
                oracle.assert_fresh(&quote, current_timestamp())?;
            }
        }
        match reservation_kind {
            ReservationKind::Proxy => self.store.reserve_v2(request)?,
            ReservationKind::OpenRouterLease => self.store.reserve_openrouter_lease(request)?,
        }
        Ok(None)
    }

    fn finalize_request(
        &self,
        request: &ApiRequestV2,
        provider_response: ProviderResponse,
        upstream_ms: u64,
        total_ms: u64,
    ) -> Result<RequestResponseV2, ServerError> {
        let public = &request.public_inputs;
        let state_key = self.state_signing_key();
        let reservation_kind = self
            .store
            .lookup_by_nullifier(&public.request_nullifier)
            .ok_or_else(|| {
                ServerError::Internal("request nullifier is no longer reserved".to_string())
            })?
            .reservation_kind;
        let policy_charged =
            self.config.policy_enabled && provider_response.policy_reason_code.is_some();
        let charge_cap = if reservation_kind == ReservationKind::OpenRouterLease.as_str() {
            public.solvency_bound
        } else if policy_charged {
            self.config.policy_charge_cap
        } else {
            self.config.request_charge_cap
        };
        if provider_response.charge_applied > charge_cap {
            return Err(ServerError::Internal(format!(
                "provider charge {} exceeds cap {}",
                provider_response.charge_applied, charge_cap
            )));
        }

        let anonymous = CurvePointWire {
            x: public.anonymous_commitment_x,
            y: public.anonymous_commitment_y,
        };
        let blind_delta = random_scalar();
        let next_commitment =
            server_update(&anonymous, provider_response.charge_applied, &blind_delta)
                .map_err(|error| ServerError::InvalidProof(error.to_string()))?;
        let next_anchor = core::next_anchor(
            &random_field(),
            &public.request_nullifier,
            &next_commitment.x,
            &next_commitment.y,
        );
        let state_message = core::state_message(
            self.config.protocol_version,
            self.config.chain_id,
            &self.config.contract_address,
            &next_commitment.x,
            &next_commitment.y,
            &next_anchor,
        );
        let state_signature = self.signer.sign_state(&state_message);
        let response_hash = canonical_response_hash(provider_response.payload.as_bytes());
        let proof_bytes = base64::engine::general_purpose::STANDARD
            .decode(&request.proof.proof)
            .map_err(|error| ServerError::InvalidProof(error.to_string()))?;
        let transcript = TranscriptRecord {
            nullifier: public.request_nullifier,
            status: NullifierStatus::Finalized,
            reservation_kind,
            client_request_id: Some(request.client_request_id.clone()),
            payload_hash: Some(request.payload_hash),
            charge_applied: Some(provider_response.charge_applied),
            response_code: Some(provider_response.status_code),
            response_payload: Some(provider_response.payload.clone()),
            response_hash: Some(response_hash),
            next_commitment_x: Some(next_commitment.x),
            next_commitment_y: Some(next_commitment.y),
            next_anchor: Some(next_anchor),
            blind_delta_srv: Some(blind_delta),
            next_state_sig_epoch: None,
            next_state_sig_root: None,
            next_state_sig: Some(state_signature),
            policy_reason_code: provider_response.policy_reason_code,
            policy_evidence_hash: provider_response.policy_evidence_hash,
            proof_blob: Some(proof_bytes.clone()),
            request_inputs_json: serde_json::to_string(public).ok(),
            api_request_binding: Some(api_request_binding(request)?),
            created_at: current_timestamp(),
            finalized_at: Some(current_timestamp()),
        };
        self.store
            .finalize(&public.request_nullifier, &transcript)?;

        if let Some(hub) = &self.dashboard {
            let (request_path, request_model, request_messages) =
                decode_request_view(&request.payload);
            hub.record(DashboardEvent {
                seq: hub.next_seq(),
                ts_ms: current_timestamp_ms(),
                client_request_id: request.client_request_id.clone(),
                billing_label: provider_response.billing_label.clone(),
                upstream_model: provider_response.upstream_model.clone(),
                request_nullifier: public.request_nullifier,
                active_root: public.active_root,
                anon_commitment: anonymous,
                solvency_bound: public.solvency_bound,
                solvency_bound_usd: if self.native_oracle.is_some() {
                    pricing::credits_to_usd(self.lease_limit_micro_usd(request)?)
                } else {
                    pricing::credits_to_usd(public.solvency_bound)
                },
                statement_type: 1,
                state_sig_epoch_in: 0,
                proof_backend: "groth16_bn254".to_string(),
                proof_public_output_hash: public.authorization_tag,
                proof_size_bytes: proof_bytes.len(),
                request_path,
                request_model,
                request_messages,
                request_raw: redact_secrets(&request.payload),
                response_code: provider_response.status_code,
                response_text: redact_secrets(&provider_response.payload),
                response_hash,
                usage: provider_response.usage.clone(),
                charge_applied: provider_response.charge_applied,
                charge_usd: if self.native_oracle.is_some() {
                    provider_response
                        .usage
                        .as_ref()
                        .map(|usage| usage.cost_usd)
                        .unwrap_or(0.0)
                } else {
                    charge_usd(provider_response.charge_applied)
                },
                next_commitment: next_commitment.clone(),
                next_anchor,
                blind_delta_srv: blind_delta,
                next_state_sig_epoch: 0,
                next_state_sig_leaf_index: 0,
                next_state_sig_root: state_key.x,
                upstream_ms,
                total_ms,
            });
        }

        Ok(RequestResponseV2 {
            status: "ok".to_string(),
            client_request_id: request.client_request_id.clone(),
            request_nullifier: public.request_nullifier,
            response_code: provider_response.status_code,
            response_payload: provider_response.payload,
            response_hash,
            charge_applied: provider_response.charge_applied,
            next_commitment,
            next_anchor,
            blind_delta_srv: blind_delta,
            next_state_signature: state_signature,
            policy_reason_code: provider_response.policy_reason_code,
            policy_evidence_hash: provider_response.policy_evidence_hash,
        })
    }

    pub fn openrouter_leases_enabled(&self) -> bool {
        self.config.openrouter_leases.is_some()
    }

    pub fn openrouter_lease_status(
        &self,
        client_request_id: &str,
    ) -> Option<OpenRouterLeaseStatusResponse> {
        self.store
            .lookup_openrouter_lease(client_request_id)
            .map(|lease| OpenRouterLeaseStatusResponse {
                status: lease.status,
                client_request_id: lease.client_request_id,
                issued_at: lease.issued_at,
                expires_at: lease.expires_at,
                settle_after: lease.settle_after,
                spending_limit_usd: lease.spending_limit_usd,
                usage_usd: lease.usage_usd,
                charge_applied: lease.charge_applied,
                last_error: lease.last_error,
            })
    }

    pub fn process_clearance(
        &self,
        request: &ClearanceRequest,
    ) -> Result<ClearanceResponseV2, ServerError> {
        let already_reserved = match self
            .store
            .lookup_by_nullifier(&request.withdrawal_nullifier)
        {
            Some(record) if record.status == NullifierStatus::ClearanceReserved => true,
            Some(_) => return Err(ServerError::NullifierUsed),
            None => false,
        };
        let message = core::clearance_message(
            self.config.protocol_version,
            self.config.chain_id,
            &self.config.contract_address,
            &request.withdrawal_nullifier,
        );
        let signature = self.signer.sign_clearance(&message);
        if !already_reserved {
            self.store
                .reserve_clearance(&request.withdrawal_nullifier)?;
        }
        Ok(ClearanceResponseV2 {
            status: "ok".to_string(),
            withdrawal_nullifier: request.withdrawal_nullifier,
            signature,
        })
    }

    pub fn recover_by_client_id(
        &self,
        client_request_id: &str,
    ) -> Result<RecoveryResponseV2, ServerError> {
        Ok(self
            .store
            .lookup_by_client_id(client_request_id)
            .map(|record| recovery_from_record(&record))
            .unwrap_or_else(not_found_recovery))
    }

    pub fn recover_by_nullifier(
        &self,
        nullifier: &Felt252,
    ) -> Result<RecoveryResponseV2, ServerError> {
        Ok(self
            .store
            .lookup_by_nullifier(nullifier)
            .map(|record| recovery_from_record(&record))
            .unwrap_or_else(not_found_recovery))
    }

    pub fn config(&self) -> &ServerConfig {
        &self.config
    }

    pub fn store(&self) -> &Arc<NullifierStore> {
        &self.store
    }
}

fn response_from_record(
    record: &TranscriptRecord,
    client_request_id: &str,
) -> Result<RequestResponseV2, ServerError> {
    Ok(RequestResponseV2 {
        status: "ok".to_string(),
        client_request_id: client_request_id.to_string(),
        request_nullifier: record.nullifier,
        response_code: record.response_code.unwrap_or(200),
        response_payload: record.response_payload.clone().unwrap_or_default(),
        response_hash: record.response_hash.unwrap_or(Felt252::ZERO),
        charge_applied: record.charge_applied.unwrap_or(0),
        next_commitment: CurvePointWire {
            x: record.next_commitment_x.unwrap_or(Felt252::ZERO),
            y: record.next_commitment_y.unwrap_or(Felt252::ZERO),
        },
        next_anchor: record.next_anchor.unwrap_or(Felt252::ZERO),
        blind_delta_srv: record.blind_delta_srv.unwrap_or(Felt252::ZERO),
        next_state_signature: record
            .next_state_sig
            .ok_or_else(|| ServerError::Internal("stored response lacks signature".to_string()))?,
        policy_reason_code: record.policy_reason_code,
        policy_evidence_hash: record.policy_evidence_hash,
    })
}

fn recovery_from_record(record: &TranscriptRecord) -> RecoveryResponseV2 {
    let status = match record.status {
        NullifierStatus::Reserved => "reserved",
        NullifierStatus::Finalized => "finalized",
        NullifierStatus::ClearanceReserved => "clearance_reserved",
    };
    RecoveryResponseV2 {
        status: "ok".to_string(),
        nullifier_status: status.to_string(),
        request_response: (record.status == NullifierStatus::Finalized)
            .then(|| {
                response_from_record(record, record.client_request_id.as_deref().unwrap_or(""))
            })
            .transpose()
            .ok()
            .flatten(),
    }
}

fn not_found_recovery() -> RecoveryResponseV2 {
    RecoveryResponseV2 {
        status: "not_found".to_string(),
        nullifier_status: "unknown".to_string(),
        request_response: None,
    }
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    use zkapi_types::wire::{Groth16ProofWire, ProofBackendWire};
    use zkapi_types::RequestPublicInputsV2;

    use crate::config::OpenRouterLeaseConfig;
    use crate::provider::EchoProvider;

    fn setup_directory() -> String {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../protocol/setup/v2")
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string()
    }

    fn oa_lease_processor(store: Arc<NullifierStore>) -> RequestProcessor {
        let state_seed = Felt252::from_u64(11);
        let clear_seed = Felt252::from_u64(12);
        let signer = Arc::new(ServerSigner::new(&state_seed, &clear_seed));
        RequestProcessor::try_new(
            ServerConfig {
                request_charge_cap: 1,
                policy_charge_cap: 1,
                proof_setup_dir: setup_directory(),
                openrouter_leases: Some(OpenRouterLeaseConfig {
                    source: OpenRouterLeaseSourceConfig::OaOrg {
                        // No request should reach this address in the mutation
                        // tests: the bound-retry check must reject first.
                        org_base_url: "http://127.0.0.1:9".to_string(),
                        shared_secret: "test-secret".to_string(),
                    },
                    ttl_seconds: 300,
                    settlement_grace_seconds: 0,
                    settlement_poll_seconds: 1,
                }),
                ..Default::default()
            },
            store,
            signer,
            Arc::new(EchoProvider::new(1)),
            Felt252::ZERO,
        )
        .unwrap()
    }

    fn unverified_lease_request(processor: &RequestProcessor) -> ApiRequestV2 {
        let payload = serde_json::to_string(&OpenRouterLeaseAuthorization::default()).unwrap();
        let state_key = processor.state_signing_key();
        ApiRequestV2 {
            client_request_id: "pre-oa-failure-request".to_string(),
            payload_hash: canonical_payload_hash(payload.as_bytes()),
            payload,
            public_inputs: RequestPublicInputsV2 {
                protocol_version: processor.config.protocol_version,
                chain_id: processor.config.chain_id,
                contract_address: processor.config.contract_address,
                active_root: Felt252::ZERO,
                state_signing_key_x: state_key.x,
                state_signing_key_y: state_key.y,
                request_time: current_timestamp(),
                solvency_bound: 3_000_000,
                request_nullifier: Felt252::from_u64(77),
                authorization_tag: Felt252::from_u64(78),
                anonymous_commitment_x: Felt252::from_u64(79),
                anonymous_commitment_y: Felt252::from_u64(80),
            },
            proof: Groth16ProofWire {
                backend: ProofBackendWire::Groth16Bn254,
                proof: "original-proof-that-was-verified-before-reservation".to_string(),
            },
        }
    }

    fn mutated_request(
        request: &ApiRequestV2,
        mutate: impl FnOnce(&mut ApiRequestV2),
    ) -> ApiRequestV2 {
        let mut mutation = request.clone();
        mutate(&mut mutation);
        mutation
    }

    #[tokio::test]
    async fn pre_oa_failure_rejects_every_mutated_reserved_retry() {
        let store = Arc::new(NullifierStore::in_memory().unwrap());
        let processor = oa_lease_processor(store.clone());
        let request = unverified_lease_request(&processor);
        let original_limit = pricing::credits_to_usd(request.public_inputs.solvency_bound);

        // This is the durable state left after proof verification and lease
        // reservation but before OA successfully returns a key.
        store.reserve_openrouter_lease(&request).unwrap();
        store
            .create_openrouter_lease(&request, "oa_org", 100, 400, 400, original_limit)
            .unwrap();

        // A byte-identical transport retry remains resumable even though its
        // proof is not verified a second time.
        assert!(processor
            .validate_and_reserve(&request, ReservationKind::OpenRouterLease)
            .unwrap()
            .is_none());

        let changed_payload = "{ \"mode\": \"openrouter_ephemeral_lease\", \"version\": 1 }";
        let binding_replay_mutations = vec![
            (
                "client_request_id",
                mutated_request(&request, |value| {
                    value.client_request_id = "different-request-id".to_string()
                }),
            ),
            (
                "payload_and_payload_hash",
                mutated_request(&request, |value| {
                    value.payload = changed_payload.to_string();
                    value.payload_hash = canonical_payload_hash(changed_payload.as_bytes());
                }),
            ),
            (
                "active_root",
                mutated_request(&request, |value| {
                    value.public_inputs.active_root = Felt252::from_u64(901)
                }),
            ),
            (
                "state_signing_key_x",
                mutated_request(&request, |value| {
                    value.public_inputs.state_signing_key_x = Felt252::from_u64(902)
                }),
            ),
            (
                "state_signing_key_y",
                mutated_request(&request, |value| {
                    value.public_inputs.state_signing_key_y = Felt252::from_u64(903)
                }),
            ),
            (
                "request_time",
                mutated_request(&request, |value| value.public_inputs.request_time = 1),
            ),
            (
                "solvency_bound",
                mutated_request(&request, |value| {
                    value.public_inputs.solvency_bound = u128::MAX
                }),
            ),
            (
                "authorization_tag",
                mutated_request(&request, |value| {
                    value.public_inputs.authorization_tag = Felt252::from_u64(904)
                }),
            ),
            (
                "anonymous_commitment_x",
                mutated_request(&request, |value| {
                    value.public_inputs.anonymous_commitment_x = Felt252::from_u64(905)
                }),
            ),
            (
                "anonymous_commitment_y",
                mutated_request(&request, |value| {
                    value.public_inputs.anonymous_commitment_y = Felt252::from_u64(906)
                }),
            ),
            (
                "proof_backend",
                mutated_request(&request, |value| {
                    value.proof.backend = ProofBackendWire::StwoCairo
                }),
            ),
            (
                "proof_string",
                mutated_request(&request, |value| {
                    value.proof.proof = "garbage-not-a-proof".to_string()
                }),
            ),
        ];
        let original_binding = api_request_binding(&request).unwrap();
        for (field, mutation) in binding_replay_mutations {
            assert_ne!(
                api_request_binding(&mutation).unwrap(),
                original_binding,
                "{field} was not covered by the complete request binding"
            );
            let result = processor.issue_openrouter_lease(&mutation).await;
            assert!(
                matches!(&result, Err(ServerError::Replay)),
                "reserved retry mutation in {field} was not rejected: {}",
                result
                    .err()
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| "unexpected success".to_string())
            );
        }

        for (field, mutation) in [
            (
                "protocol_version",
                mutated_request(&request, |value| value.public_inputs.protocol_version += 1),
            ),
            (
                "chain_id",
                mutated_request(&request, |value| value.public_inputs.chain_id += 1),
            ),
            (
                "contract_address",
                mutated_request(&request, |value| {
                    value.public_inputs.contract_address = Felt252::from_u64(907)
                }),
            ),
        ] {
            assert_ne!(api_request_binding(&mutation).unwrap(), original_binding);
            let error = processor
                .issue_openrouter_lease(&mutation)
                .await
                .err()
                .unwrap_or_else(|| panic!("{field} mutation unexpectedly succeeded"));
            assert!(
                matches!(&error, ServerError::ProtocolMismatch(_)),
                "{field} mutation returned {error}"
            );
        }

        let payload_only = mutated_request(&request, |value| {
            value.payload = changed_payload.to_string()
        });
        let payload_hash_only = mutated_request(&request, |value| {
            value.payload_hash = Felt252::from_u64(908)
        });
        for (field, mutation) in [
            ("payload", payload_only),
            ("payload_hash", payload_hash_only),
        ] {
            assert_ne!(api_request_binding(&mutation).unwrap(), original_binding);
            let error = processor
                .issue_openrouter_lease(&mutation)
                .await
                .err()
                .unwrap_or_else(|| panic!("{field} mutation unexpectedly succeeded"));
            assert!(
                matches!(&error, ServerError::InvalidRequest(_)),
                "{field} mutation returned {error}"
            );
        }

        let changed_nullifier = mutated_request(&request, |value| {
            value.public_inputs.request_nullifier = Felt252::from_u64(909)
        });
        assert_ne!(
            api_request_binding(&changed_nullifier).unwrap(),
            original_binding
        );
        let error = processor
            .issue_openrouter_lease(&changed_nullifier)
            .await
            .err()
            .expect("request_nullifier mutation unexpectedly succeeded");
        assert!(
            matches!(&error, ServerError::InvalidProof(_)),
            "request_nullifier mutation returned {error}"
        );

        assert!(matches!(
            processor.validate_and_reserve(&request, ReservationKind::Proxy),
            Err(ServerError::Replay)
        ));

        let lease = store
            .lookup_openrouter_lease(&request.client_request_id)
            .unwrap();
        assert_eq!(
            lease.api_request.public_inputs.solvency_bound,
            request.public_inputs.solvency_bound
        );
        assert_eq!(lease.spending_limit_usd.to_bits(), original_limit.to_bits());
        assert_eq!(lease.status, "provisioning");
        assert!(lease.key_hash.is_none());
    }
    #[test]
    fn native_oa_namespace_is_deployment_and_full_request_bound_and_persisted() {
        let store = Arc::new(NullifierStore::in_memory().unwrap());
        let mut processor = oa_lease_processor(store.clone());
        let request = unverified_lease_request(&processor);
        let id = RequestProcessor::native_oa_request_id(&request).unwrap();
        assert_eq!(
            id,
            RequestProcessor::native_oa_request_id(&request.clone()).unwrap()
        );
        assert_eq!(id.len(), 73);
        for mutate in [
            (|r: &mut ApiRequestV2| r.public_inputs.chain_id += 1) as fn(&mut ApiRequestV2),
            |r| r.public_inputs.contract_address = Felt252::from_u64(999),
            |r| r.public_inputs.request_nullifier = Felt252::from_u64(998),
            |r| r.proof.proof.push('x'),
            |r| r.client_request_id.push('x'),
        ] {
            let mut different = request.clone();
            mutate(&mut different);
            assert_ne!(
                id,
                RequestProcessor::native_oa_request_id(&different).unwrap()
            );
        }
        store.reserve_openrouter_lease(&request).unwrap();
        store
            .create_openrouter_lease_with_oa_id(&request, "oa_org", 1, 2, 3, 1.0, Some(&id))
            .unwrap();
        let saved = store
            .lookup_openrouter_lease(&request.client_request_id)
            .unwrap();
        assert_eq!(saved.oa_client_request_id.as_deref(), Some(id.as_str()));
        assert_eq!(saved.client_request_id, request.client_request_id);
        assert_eq!(processor.persisted_oa_request_id(&saved).unwrap(), id);
        processor.config.native_billing = Some(crate::native_billing::NativeBillingConfig {
            rpc_url: "http://127.0.0.1:9".into(),
            feed_address: "0x694aa1769357215de4fac081bf1f309adc325306".into(),
            decimals: 8,
            max_age_seconds: 4500,
        });
        assert_eq!(processor.persisted_oa_request_id(&saved).unwrap(), id);
        let mut corrupted = saved.clone();
        corrupted.oa_client_request_id = None;
        assert!(processor.persisted_oa_request_id(&corrupted).is_err());
        corrupted.oa_client_request_id = Some("different-deployment".into());
        assert!(processor.persisted_oa_request_id(&corrupted).is_err());
        processor.config.native_billing = None;
        corrupted.oa_client_request_id = None;
        assert_eq!(
            processor.persisted_oa_request_id(&corrupted).unwrap(),
            request.client_request_id
        );
    }

    #[tokio::test]
    async fn native_oa_issuance_and_usage_use_same_persisted_upstream_namespace() {
        use axum::{routing::post, Json, Router};
        use serde_json::{json, Value};
        let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let issuance_seen = seen.clone();
        let usage_seen = seen.clone();
        let now = current_timestamp();
        let app = Router::new()
            .route("/api/zkapi/request_key", post(move |Json(body): Json<Value>| {
                let seen = issuance_seen.clone();
                async move {
                    seen.lock().unwrap().push(body["client_request_id"].as_str().unwrap().into());
                    Json(json!({"source":"oa_org", "key":"runtime-key", "key_hash":"key-hash",
                        "credit_limit":1.0, "duration_minutes":5, "expires_at_unix":now+300,
                        "station_id":"station", "station_recently_attested":true,
                        "station_signature":"ab".repeat(64), "org_signature":"cd".repeat(64),
                        "verifier_url":"https://verifier.example", "openrouter_api_base":"https://openrouter.ai/api/v1"}))
                }
            }))
            .route("/api/zkapi/key_usage", post(move |Json(body): Json<Value>| {
                let seen = usage_seen.clone();
                async move {
                    seen.lock().unwrap().push(body["client_request_id"].as_str().unwrap().into());
                    Json(json!({"source":"oa_org", "version":1, "status":"pending",
                        "client_request_id":body["client_request_id"], "key_hash":"key-hash",
                        "station_request_id":"ab".repeat(32), "retry_after_seconds":1}))
                }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let store = Arc::new(NullifierStore::in_memory().unwrap());
        let mut processor = oa_lease_processor(store.clone());
        processor.oa_org = Some(Arc::new(
            OaOrgProvisioner::new(url, "test-secret".into()).unwrap(),
        ));
        processor.config.native_billing = Some(crate::native_billing::NativeBillingConfig {
            rpc_url: "http://127.0.0.1:9".into(),
            feed_address: "0x694aa1769357215de4fac081bf1f309adc325306".into(),
            decimals: 8,
            max_age_seconds: 4500,
        });
        let mut request = unverified_lease_request(&processor);
        request.payload = json!({"mode":"openrouter_ephemeral_lease","version":1,"billing_quote":{
            "asset":"native_eth","units_per_eth":1_000_000_000,"chain_id":1,
            "feed_address":"0x694aa1769357215de4fac081bf1f309adc325306","round_id":"123",
            "answer":"250000000000","decimals":8,"updated_at":now,"expires_at":now+4500
        }})
        .to_string();
        request.payload_hash = canonical_payload_hash(request.payload.as_bytes());
        request.public_inputs.solvency_bound = 400_000;
        let id = RequestProcessor::native_oa_request_id(&request).unwrap();
        // Resume at the already-verified durable boundary, avoiding mock proofs
        // at admission; the real-proof tests separately cover that boundary.
        store.reserve_openrouter_lease(&request).unwrap();
        store
            .create_openrouter_lease_with_oa_id(
                &request,
                "oa_org",
                now,
                now + 300,
                now + 300,
                1.0,
                Some(&id),
            )
            .unwrap();
        let issued = processor.issue_openrouter_lease(&request).await.unwrap();
        assert_eq!(issued.lease.client_request_id, request.client_request_id);
        let saved = store
            .lookup_openrouter_lease(&request.client_request_id)
            .unwrap();
        assert!(matches!(
            processor.settle_oa_org_lease(&saved).await,
            Err(ServerError::LeaseSettlementPending { .. })
        ));
        assert_eq!(*seen.lock().unwrap(), vec![id.clone(), id]);
        assert_eq!(
            store
                .lookup_openrouter_lease(&request.client_request_id)
                .unwrap()
                .status,
            "active"
        );
        server.abort();
    }

    #[tokio::test]
    async fn native_lease_persists_frozen_quote_and_rejects_missing_or_changed_quotes() {
        use crate::native_billing::NativeBillingConfig;
        let store = Arc::new(NullifierStore::in_memory().unwrap());
        let mut processor = oa_lease_processor(store.clone());
        processor.config.contract_address = Felt252::from_u64(1);
        let config = NativeBillingConfig {
            rpc_url: "http://127.0.0.1:9".into(),
            feed_address: "0x694aa1769357215de4fac081bf1f309adc325306".into(),
            decimals: 8,
            max_age_seconds: 3600,
        };
        processor.config.native_billing = Some(config.clone());
        processor.native_oracle = Some(
            NativeBillingOracle::new(
                config,
                1,
                "0x0000000000000000000000000000000000000001".into(),
            )
            .unwrap(),
        );
        let mut request = unverified_lease_request(&processor);
        assert!(processor.lease_authorization(&request).is_err());
        let quote = NativeBillingQuote {
            asset: "native_eth".into(),
            units_per_eth: 1_000_000_000,
            chain_id: 1,
            feed_address: "0x694aa1769357215de4fac081bf1f309adc325306".into(),
            round_id: "123".into(),
            answer: "250000000000".into(),
            decimals: 8,
            updated_at: 100,
            expires_at: 3700,
        };
        request.payload = serde_json::json!({"mode":"openrouter_ephemeral_lease","version":1,"billing_quote":quote}).to_string();
        request.payload_hash = canonical_payload_hash(request.payload.as_bytes());
        request.public_inputs.solvency_bound = 400_000;
        assert_eq!(
            processor.lease_limit_micro_usd(&request).unwrap(),
            1_000_000
        );
        assert_eq!(
            processor.lease_charge_units(&request, 123_456).unwrap(),
            49_383
        );
        assert!(matches!(
            processor.issue_openrouter_lease(&request).await,
            Err(ServerError::NativeQuoteExpired)
        ));
        assert!(store
            .lookup_by_nullifier(&request.public_inputs.request_nullifier)
            .is_none());
        let expired = processor
            .expire_unaccepted_native_lease(&request.client_request_id, &request)
            .await
            .unwrap();
        assert_eq!(expired["status"], "expired_unaccepted");
        assert_eq!(
            expired["payload_hash"],
            serde_json::to_value(request.payload_hash).unwrap()
        );
        assert_eq!(
            expired["request_nullifier"],
            serde_json::to_value(request.public_inputs.request_nullifier).unwrap()
        );
        assert!(store
            .lookup_by_nullifier(&request.public_inputs.request_nullifier)
            .is_none());
        // An expiry check must wait behind in-flight issuance. Simulate that
        // issuer accepting the request before it releases the shared lock.
        let issuance_guard = processor.lease_issue_lock.lock().await;
        let expiry_check =
            processor.expire_unaccepted_native_lease(&request.client_request_id, &request);
        tokio::pin!(expiry_check);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(25), &mut expiry_check)
                .await
                .is_err()
        );
        store.reserve_openrouter_lease(&request).unwrap();
        drop(issuance_guard);
        assert!(matches!(expiry_check.await, Err(ServerError::LeasePending)));
        store
            .create_openrouter_lease_with_oa_id(
                &request,
                "oa_org",
                100,
                400,
                400,
                1.0,
                Some(&RequestProcessor::native_oa_request_id(&request).unwrap()),
            )
            .unwrap();
        let restored = store
            .lookup_openrouter_lease(&request.client_request_id)
            .unwrap();
        assert_eq!(
            processor
                .lease_authorization(&restored.api_request)
                .unwrap()
                .1,
            Some(quote.clone())
        );
        // Retry must reach the unavailable issuer, not fetch/reprice an expired
        // round. This confirms that a restarted lease uses its persisted quote.
        let error = processor
            .issue_openrouter_lease(&restored.api_request)
            .await
            .err()
            .unwrap();
        assert!(
            !error.to_string().contains("stale") && !error.to_string().contains("oracle"),
            "{error}"
        );
        let mut collision = restored.api_request.clone();
        collision.public_inputs.request_nullifier = Felt252::from_u64(999);
        assert!(matches!(
            processor.issue_openrouter_lease(&collision).await,
            Err(ServerError::Replay)
        ));
        assert!(store
            .lookup_by_nullifier(&collision.public_inputs.request_nullifier)
            .is_none());
        let mut mutation = restored.api_request.clone();
        let mut payload: serde_json::Value = serde_json::from_str(&mutation.payload).unwrap();
        payload["billing_quote"]["answer"] = "500000000000".into();
        mutation.payload = payload.to_string();
        mutation.payload_hash = canonical_payload_hash(mutation.payload.as_bytes());
        assert!(matches!(
            processor.issue_openrouter_lease(&mutation).await,
            Err(ServerError::Replay)
        ));
        assert!(processor.process_request(&request).await.is_err());
        let legacy = oa_lease_processor(Arc::new(NullifierStore::in_memory().unwrap()));
        assert!(legacy.lease_authorization(&request).is_err());
    }
    #[tokio::test]
    async fn native_issuance_rechecks_expiry_after_delayed_oracle_read() {
        use crate::native_billing::NativeBillingConfig;
        use axum::{routing::post, Json, Router};
        use serde_json::{json, Value};
        use std::time::Duration;
        use tokio::sync::Notify;

        let expires_at = current_timestamp() + 5;
        let updated_at = expires_at - 3600;
        let oracle_read_started = Arc::new(Notify::new());
        let finish_oracle_read = Arc::new(Notify::new());
        let started = oracle_read_started.clone();
        let finish = finish_oracle_read.clone();
        let app = Router::new().route(
            "/",
            post(move |Json(request): Json<Value>| {
                let started = started.clone();
                let finish = finish.clone();
                async move {
                    let data = request["params"][0]["data"].as_str().unwrap_or("");
                    let result = if request["method"] == "eth_chainId" {
                        "0x1".to_string()
                    } else if data == "0x313ce567" {
                        format!("0x{:064x}", 8)
                    } else if data == "0x4d1352fd" {
                        format!("0x{:064x}", 1_000_000_000)
                    } else {
                        assert_eq!(request["params"][1], "finalized");
                        if data.starts_with("0x9a6fc8f5") {
                            started.notify_one();
                            finish.notified().await;
                        } else {
                            assert_eq!(data, "0xfeaf968c");
                        }
                        format!(
                            "0x{:064x}{:064x}{:064x}{:064x}{:064x}",
                            123, 250_000_000_000u64, updated_at, updated_at, 123
                        )
                    };
                    Json(json!({"jsonrpc":"2.0","id":1,"result":result}))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let rpc_url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let store = Arc::new(NullifierStore::in_memory().unwrap());
        let mut processor = oa_lease_processor(store.clone());
        processor.config.contract_address = Felt252::from_u64(1);
        let config = NativeBillingConfig {
            rpc_url,
            feed_address: "0x694aa1769357215de4fac081bf1f309adc325306".into(),
            decimals: 8,
            max_age_seconds: 3600,
        };
        processor.config.native_billing = Some(config.clone());
        processor.native_oracle = Some(
            NativeBillingOracle::new(
                config,
                1,
                "0x0000000000000000000000000000000000000001".into(),
            )
            .unwrap(),
        );
        let processor = Arc::new(processor);
        let mut request = unverified_lease_request(&processor);
        let quote = NativeBillingQuote {
            asset: "native_eth".into(),
            units_per_eth: 1_000_000_000,
            chain_id: 1,
            feed_address: "0x694aa1769357215de4fac081bf1f309adc325306".into(),
            round_id: "123".into(),
            answer: "250000000000".into(),
            decimals: 8,
            updated_at,
            expires_at,
        };
        request.payload =
            json!({"mode":"openrouter_ephemeral_lease","version":1,"billing_quote":quote})
                .to_string();
        request.payload_hash = canonical_payload_hash(request.payload.as_bytes());
        request.public_inputs.solvency_bound = 400_000;
        let issuer = processor.clone();
        let issued_request = request.clone();
        let issuance =
            tokio::spawn(async move { issuer.issue_openrouter_lease(&issued_request).await });
        tokio::time::timeout(Duration::from_secs(10), oracle_read_started.notified())
            .await
            .unwrap();
        while current_timestamp() < expires_at {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        finish_oracle_read.notify_one();
        let result = tokio::time::timeout(Duration::from_secs(5), issuance)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(result, Err(ServerError::NativeQuoteExpired)));
        assert!(store
            .lookup_by_nullifier(&request.public_inputs.request_nullifier)
            .is_none());
        assert!(store
            .lookup_openrouter_lease(&request.client_request_id)
            .is_none());
        assert_eq!(
            processor
                .expire_unaccepted_native_lease(&request.client_request_id, &request)
                .await
                .unwrap()["status"],
            "expired_unaccepted"
        );
        server.abort();
    }
    #[tokio::test]
    async fn native_issuance_uses_latest_finalized_round_and_recovers_superseded_request() {
        use crate::native_billing::NativeBillingConfig;
        use axum::{routing::post, Json, Router};
        use serde_json::{json, Value};
        use std::sync::atomic::{AtomicU64, Ordering};

        let updated_at = current_timestamp() - 30;
        let finalized_round = Arc::new(AtomicU64::new(123));
        let head_round = Arc::new(AtomicU64::new(124));
        let rpc_finalized = finalized_round.clone();
        let rpc_head = head_round.clone();
        let app = Router::new().route(
            "/",
            post(move |Json(request): Json<Value>| {
                let finalized = rpc_finalized.clone();
                let head = rpc_head.clone();
                async move {
                    let data = request["params"][0]["data"].as_str().unwrap_or("");
                    let result = if request["method"] == "eth_chainId" {
                        "0x1".to_string()
                    } else if data == "0x313ce567" {
                        format!("0x{:064x}", 8)
                    } else if data == "0x4d1352fd" {
                        format!("0x{:064x}", 1_000_000_000)
                    } else {
                        let latest = if request["params"][1] == "finalized" {
                            finalized.load(Ordering::SeqCst)
                        } else {
                            head.load(Ordering::SeqCst)
                        };
                        let round = if data == "0xfeaf968c" {
                            latest
                        } else {
                            assert!(data.starts_with("0x9a6fc8f5"));
                            let round = u64::from_str_radix(&data[10..], 16).unwrap();
                            assert!(round <= latest);
                            round
                        };
                        let price = if round == 123 {
                            250_000_000_000u64
                        } else {
                            200_000_000_000u64
                        };
                        format!(
                            "0x{:064x}{:064x}{:064x}{:064x}{:064x}",
                            round, price, updated_at, updated_at, round
                        )
                    };
                    Json(json!({"jsonrpc":"2.0","id":1,"result":result}))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let rpc_url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let store = Arc::new(NullifierStore::in_memory().unwrap());
        let mut processor = oa_lease_processor(store.clone());
        processor.config.contract_address = Felt252::from_u64(1);
        let config = NativeBillingConfig {
            rpc_url,
            feed_address: "0x694aa1769357215de4fac081bf1f309adc325306".into(),
            decimals: 8,
            max_age_seconds: 4500,
        };
        processor.config.native_billing = Some(config.clone());
        processor.native_oracle = Some(
            NativeBillingOracle::new(
                config,
                1,
                "0x0000000000000000000000000000000000000001".into(),
            )
            .unwrap(),
        );
        let oracle = processor.native_oracle.as_ref().unwrap();
        let quote = oracle.quote(current_timestamp()).await.unwrap();
        assert_eq!(
            quote.round_id, "123",
            "head-only round must not influence pricing"
        );
        oracle.validate(&quote, current_timestamp()).await.unwrap();
        let mut request = unverified_lease_request(&processor);
        request.payload =
            json!({"mode":"openrouter_ephemeral_lease","version":1,"billing_quote":quote})
                .to_string();
        request.payload_hash = canonical_payload_hash(request.payload.as_bytes());
        request.public_inputs.solvency_bound = 400_000;
        assert!(matches!(
            processor
                .expire_unaccepted_native_lease(&request.client_request_id, &request)
                .await,
            Err(ServerError::LeasePending)
        ));
        // Proof construction used round123. Once round124 finalizes, a new
        // issuance cannot choose the more favorable historical price123.
        finalized_round.store(124, Ordering::SeqCst);
        assert!(matches!(
            processor.issue_openrouter_lease(&request).await,
            Err(ServerError::NativeQuoteSuperseded)
        ));
        assert!(store
            .lookup_by_nullifier(&request.public_inputs.request_nullifier)
            .is_none());
        let ack = processor
            .expire_unaccepted_native_lease(&request.client_request_id, &request)
            .await
            .unwrap();
        assert_eq!(ack["status"], "superseded_unaccepted");
        assert_eq!(
            ack["payload_hash"],
            serde_json::to_value(request.payload_hash).unwrap()
        );
        assert_eq!(
            ack["request_nullifier"],
            serde_json::to_value(request.public_inputs.request_nullifier).unwrap()
        );
        // A normal head reorg cannot revive an acknowledged old finalized quote.
        head_round.store(123, Ordering::SeqCst);
        assert!(matches!(
            processor.issue_openrouter_lease(&request).await,
            Err(ServerError::NativeQuoteSuperseded)
        ));
        // Simulate a previously accepted request from before finality advanced:
        // recovery never discards it and measured usage uses its frozen price.
        store.reserve_openrouter_lease(&request).unwrap();
        store
            .create_openrouter_lease_with_oa_id(
                &request,
                "oa_org",
                updated_at,
                updated_at + 300,
                updated_at + 300,
                1.0,
                Some(&RequestProcessor::native_oa_request_id(&request).unwrap()),
            )
            .unwrap();
        assert!(matches!(
            processor
                .expire_unaccepted_native_lease(&request.client_request_id, &request)
                .await,
            Err(ServerError::LeasePending)
        ));
        assert_eq!(
            processor.lease_charge_units(&request, 123_456).unwrap(),
            49_383
        );
        let error = processor
            .issue_openrouter_lease(&request)
            .await
            .err()
            .unwrap();
        assert!(!matches!(
            error,
            ServerError::NativeQuoteSuperseded | ServerError::NativeQuoteExpired
        ));
        server.abort();
    }
    #[test]
    fn native_quote_is_bound_by_real_browser_proof_and_settles_in_gwei() {
        use zkapi_browser::{
            BrowserWalletConfig, CompleteResponseArgs, ConfirmDepositArgs, PrepareRequestArgs,
        };
        use zkapi_core::merkle::MerkleTree;
        let store = Arc::new(NullifierStore::in_memory().unwrap());
        let mut processor = oa_lease_processor(store);
        processor.config.contract_address = Felt252::from_u64(1);
        processor.config.request_charge_cap = 400_000;
        processor.config.native_billing = Some(crate::native_billing::NativeBillingConfig {
            rpc_url: "http://127.0.0.1:9".into(),
            feed_address: "0x694aa1769357215de4fac081bf1f309adc325306".into(),
            decimals: 8,
            max_age_seconds: 3600,
        });
        let config = BrowserWalletConfig {
            protocol_version: 2,
            chain_id: 1,
            contract_address: processor.config.contract_address,
            request_charge_cap: 400_000,
            policy_charge_cap: 400_000,
            policy_enabled: false,
            state_signing_key: processor.state_signing_key(),
            clearance_signing_key: processor.clearance_signing_key(),
        };
        let params = zkapi_browser::generate_deposit_params();
        let state = zkapi_browser::confirm_deposit(
            &config,
            ConfirmDepositArgs {
                secret: params.secret,
                note_id: 0,
                amount: 2_000_000,
                expiry_ts: 4_000_000_000,
            },
        )
        .unwrap();
        let mut tree = MerkleTree::new();
        tree.set_leaf(
            0,
            core::note_leaf(0, &params.registration_commitment, 2_000_000, 4_000_000_000),
        );
        processor.update_root(tree.root());
        let now = current_timestamp();
        let quote = NativeBillingQuote {
            asset: "native_eth".into(),
            units_per_eth: 1_000_000_000,
            chain_id: 1,
            feed_address: "0x694aa1769357215de4fac081bf1f309adc325306".into(),
            round_id: "123".into(),
            answer: "250000000000".into(),
            decimals: 8,
            updated_at: now,
            expires_at: now + 3600,
        };
        let payload=serde_json::json!({"mode":"openrouter_ephemeral_lease","version":1,"billing_quote":quote}).to_string();
        let key =
            std::fs::read(std::path::Path::new(&setup_directory()).join("request.pk")).unwrap();
        let prepared = zkapi_browser::prepare_request(
            &config,
            &state,
            PrepareRequestArgs {
                payload: payload.clone(),
                active_root: tree.root(),
                merkle_siblings: tree.get_siblings(0).to_vec(),
                client_request_id: "native-real-proof".into(),
                request_time: now,
                created_at_ms: now * 1000,
            },
            &key,
        )
        .unwrap();
        assert_eq!(prepared.request.payload, payload);
        let serialized = serde_json::to_string(&prepared.journal).unwrap();
        let journal: zkapi_browser::PendingRequestJournal =
            serde_json::from_str(&serialized).unwrap();
        assert_eq!(journal.prepared_request.payload, payload);
        let mut changed = prepared.request.clone();
        let mut changed_payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
        changed_payload["billing_quote"]["answer"] = "500000000000".into();
        changed.payload = changed_payload.to_string();
        changed.payload_hash = canonical_payload_hash(changed.payload.as_bytes());
        assert!(matches!(
            processor.validate_and_reserve(&changed, ReservationKind::OpenRouterLease),
            Err(ServerError::InvalidProof(_))
        ));
        let context = canonical_request_context(&changed.client_request_id, &changed.payload_hash);
        changed.public_inputs.authorization_tag =
            core::authorization_tag(&changed.public_inputs.request_nullifier, &context);
        assert!(matches!(
            processor.validate_and_reserve(&changed, ReservationKind::OpenRouterLease),
            Err(ServerError::InvalidProof(_))
        ));
        assert!(processor
            .validate_and_reserve(&prepared.request, ReservationKind::OpenRouterLease)
            .unwrap()
            .is_none());
        let charge = processor
            .lease_charge_units(&prepared.request, 123_456)
            .unwrap();
        assert_eq!(charge, 49_383);
        let response = processor
            .finalize_request(
                &prepared.request,
                ProviderResponse {
                    status_code: 200,
                    payload: serde_json::json!({"billing_quote":quote,"usage_credits":123456})
                        .to_string(),
                    charge_applied: charge,
                    policy_reason_code: None,
                    policy_evidence_hash: None,
                    usage: None,
                    upstream_model: None,
                    billing_label: "native-test".into(),
                },
                0,
                0,
            )
            .unwrap();
        let next = zkapi_browser::complete_response(
            &config,
            CompleteResponseArgs {
                state,
                journal,
                response,
            },
        )
        .unwrap();
        assert_eq!(next.current_balance, 2_000_000 - charge);
    }
}
