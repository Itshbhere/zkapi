export const nativeFunding = (changes = {}) => ({
    billing_asset: 'native_eth', billing_unit: 'gwei', chain_id: 11155111,
    contract_address: `0x${'22'.repeat(20)}`, native_asset_wei_per_unit: '1000000000',
    native_price_feed_address: `0x${'33'.repeat(20)}`, native_price_feed_decimals: 8,
    native_price_max_age_seconds: 3600, demo_rpc_url: 'https://rpc.example',
    protocol_server_url: 'https://protocol.example', ...changes
});

// $1,000/ETH keeps the existing ledger-size fixtures exact: $1 = 1M gwei.
export const nativeQuote = (funding = nativeFunding()) => {
    const updated = Math.floor(Date.now() / 1000) - 60;
    return { asset: 'native_eth', units_per_eth: 1_000_000_000,
        chain_id: funding.chain_id, feed_address: funding.native_price_feed_address,
        round_id: '123', answer: '100000000000', decimals: 8,
        updated_at: updated, expires_at: updated + funding.native_price_max_age_seconds };
};
