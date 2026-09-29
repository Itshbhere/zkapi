//! Upstream USD accounting for native ETH lease settlement.
//!
//! These units describe provider spend. NativeBillingQuote performs the
//! separate conversion between micro-USD and the vault's whole-gwei ledger.

const MICRO_USD_PER_USD: f64 = 1_000_000.0;

/// Round upstream USD spend up to a whole micro-dollar before converting to gwei.
pub fn usd_to_micro_usd(cost_usd: f64) -> u128 {
    if !cost_usd.is_finite() || cost_usd <= 0.0 {
        return 0;
    }
    (cost_usd * MICRO_USD_PER_USD).ceil().max(1.0) as u128
}

pub fn micro_usd_to_usd(micro_usd: u128) -> f64 {
    micro_usd as f64 / MICRO_USD_PER_USD
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_cost_rounds_up_with_floor() {
        assert_eq!(usd_to_micro_usd(0.0000051), 6);
        assert_eq!(usd_to_micro_usd(0.000010), 10);
        assert_eq!(usd_to_micro_usd(0.0000001), 1);
        assert_eq!(usd_to_micro_usd(0.0), 0);
        assert_eq!(usd_to_micro_usd(-1.0), 0);
    }

    #[test]
    fn upstream_cost_round_trip() {
        assert!((micro_usd_to_usd(1_000_000) - 1.0).abs() < 1e-12);
        assert!((micro_usd_to_usd(6) - 0.000006).abs() < 1e-12);
    }
}
