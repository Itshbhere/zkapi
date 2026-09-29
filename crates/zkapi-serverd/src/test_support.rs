//! Native lease fixtures; never compiled into the server binary.
use crate::config::{OpenRouterLeaseConfig, OpenRouterLeaseSourceConfig, ServerConfig};
use crate::native_billing::NativeBillingConfig;

pub(crate) fn native_config() -> NativeBillingConfig {
    NativeBillingConfig {
        rpc_url: "http://127.0.0.1:9".into(),
        feed_address: "0x694aa1769357215de4fac081bf1f309adc325306".into(),
        decimals: 8,
        max_age_seconds: 4500,
    }
}

pub(crate) fn lease_config() -> OpenRouterLeaseConfig {
    OpenRouterLeaseConfig {
        source: OpenRouterLeaseSourceConfig::OaOrg {
            org_base_url: "http://127.0.0.1:9".into(),
            shared_secret: "test-secret".into(),
        },
        ttl_seconds: 300,
        settlement_grace_seconds: 0,
        settlement_poll_seconds: 1,
    }
}

pub(crate) fn lease_payload(config: &ServerConfig, now: u64) -> String {
    let native = config.native_billing.as_ref().unwrap();
    // A fixed $1000/ETH test quote makes one micro-USD equal one gwei.
    serde_json::json!({"mode":"openrouter_ephemeral_lease","version":1,"billing_quote":{
        "asset":"native_eth", "units_per_eth":1_000_000_000, "chain_id":config.chain_id,
        "feed_address":native.feed_address, "round_id":"123", "answer":"100000000000",
        "decimals":native.decimals, "updated_at":now, "expires_at":now+native.max_age_seconds,
    }})
    .to_string()
}
