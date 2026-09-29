import test from 'node:test';
import assert from 'node:assert/strict';
import { TestnetPasswordAuth, TESTNET_PASSWORD_HEADER } from '../services/testnetPasswordAuth.mjs';
import { BrowserWalletRuntime } from '../services/browserWalletRuntime.js';
import runtime from '../services/browserWalletRuntime.js';
import { ZkapiClient } from '../services/zkapiClient.js';

const server = 'https://sepolia.example';
const funding = { chain_id: 11155111, protocol_server_url: server };
const json = (value, status = 200) => new Response(JSON.stringify(value), { status });
function fixture({ chainId = 11155111, required = true, requestPassword } = {}) {
    const calls = [];
    const auth = new TestnetPasswordAuth({ funding: { ...funding, chain_id: chainId }, requestPassword,
        fetch: async (url, init) => {
            calls.push({ url, init });
            if (url.endsWith('/health')) return json({ chain_id: chainId, testnet_password_required: required });
            if (url.endsWith('/v2/auth')) return new Headers(init.headers).get(TESTNET_PASSWORD_HEADER) === 'shared-test-password'
                ? json({ authenticated: true }) : json({ error_code: 'testnet_password_required' }, 401);
            return json({ ok: true });
        } });
    return { auth, calls };
}

test('Sepolia background requests fail before proof transmission and never prompt', async () => {
    const { auth, calls } = fixture({ requestPassword: () => assert.fail('background must not prompt') });
    await assert.rejects(auth.fetch(`${server}/v2/openrouter/leases`, { method: 'POST', body: 'proof' }), { code: 'testnet_password_required' });
    assert.deepEqual(calls.map(call => call.url), [`${server}/health`]);
    assert.equal(new Headers(calls[0].init.headers).has(TESTNET_PASSWORD_HEADER), false);
});

test('accepted password stays out of serializable state and is sent only to pinned Sepolia v2 routes', async () => {
    const { auth, calls } = fixture({ requestPassword: async ({ authenticate }) => {
        await assert.rejects(authenticate('wrong'), { code: 'testnet_password_required' });
        await authenticate('shared-test-password');
    } });
    await auth.ensure({ interactive: true });
    assert.equal(auth.authenticated, true);
    assert.doesNotMatch(JSON.stringify(auth), /shared-test-password/);
    await auth.fetch(`${server}/v2/openrouter/leases`, { credentials: 'include', redirect: 'follow', method: 'POST', body: 'proof' });
    const sent = calls.at(-1);
    assert.equal(new Headers(sent.init.headers).get(TESTNET_PASSWORD_HEADER), 'shared-test-password');
    assert.equal(sent.init.credentials, 'omit');
    assert.equal(sent.init.redirect, 'error');
    assert.equal(sent.init.body, 'proof');
    for (const url of [
        `${server}/health`, `${server}/config.json`, `${server}/indexer/root`,
        `${server}/v2evil/requests`, 'https://rpc.example', 'https://openrouter.ai/api/v1/chat/completions',
        'https://org.example/auth/session', 'https://sepolia.example.attacker.test/v2/auth'
    ]) {
        await auth.fetch(url, { headers: { [TESTNET_PASSWORD_HEADER]: 'caller-secret' } });
        assert.equal(new Headers(calls.at(-1).init.headers).has(TESTNET_PASSWORD_HEADER), false, url);
    }
});

test('mainnet never discovers, prompts or attaches testnet credentials', async () => {
    const { auth, calls } = fixture({ chainId: 1, requestPassword: () => assert.fail('Mainnet must not prompt') });
    await auth.ensure({ interactive: true });
    await auth.validate('shared-test-password');
    assert.equal(calls.length, 0);
    await auth.fetch(`${server}/v2/openrouter/leases`, { headers: { [TESTNET_PASSWORD_HEADER]: 'caller-secret' } });
    assert.equal(calls.length, 1);
    assert.equal(new Headers(calls[0].init.headers).has(TESTNET_PASSWORD_HEADER), false);
});

test('401 clears the credential and does not replay the original mutation', async () => {
    const { auth, calls } = fixture({ requestPassword: async ({ authenticate }) => authenticate('shared-test-password') });
    await auth.ensure({ interactive: true });
    let mutations = 0;
    const fetch = auth.fetchRaw;
    auth.fetchRaw = (url, init) => url.endsWith('/leases')
        ? (++mutations, json({ error_code: 'testnet_password_required' }, 401)) : fetch(url, init);
    assert.equal((await auth.fetch(`${server}/v2/openrouter/leases`, { method: 'POST', body: 'saved-proof' })).status, 401);
    assert.equal(mutations, 1);
    assert.equal(auth.authenticated, false);
    await auth.ensure({ interactive: true });
    assert.equal(auth.authenticated, true);
    assert.equal(mutations, 1);
    assert.equal(calls.filter(call => call.url.endsWith('/v2/auth')).length, 2);
});

test('invalid or failed health discovery fails closed; disabled Sepolia is supported', async () => {
    const { auth } = fixture();
    for (const response of [json({ chain_id: 1, testnet_password_required: false }), json({ chain_id: 11155111 }), json({}, 503)]) {
        auth.fetchRaw = async () => response;
        await assert.rejects(auth.ensure({ interactive: true }), /Sepolia/);
    }
    const disabled = fixture({ required: false });
    await disabled.auth.ensure();
    assert.equal(disabled.auth.authenticated, false);
});

test('canceled password validation cannot install a credential after the dialog closes', async () => {
    const controller = new AbortController();
    const { auth } = fixture({ requestPassword: async ({ authenticate }) => {
        const checking = authenticate('shared-test-password', { signal: controller.signal });
        controller.abort();
        await checking;
    } });
    await assert.rejects(auth.ensure({ interactive: true }), { name: 'AbortError' });
    assert.equal(auth.authenticated, false);
});

test('a canceled concurrent waiter leaves the other caller password prompt usable', async () => {
    let unlock;
    let ready;
    const visible = new Promise(resolve => { ready = resolve; });
    const { auth } = fixture({ requestPassword: async ({ authenticate }) => {
        ready();
        await new Promise(resolve => { unlock = resolve; });
        await authenticate('shared-test-password');
    } });
    const first = auth.ensure({ interactive: true });
    await visible;
    const controller = new AbortController();
    const second = auth.ensure({ interactive: true, signal: controller.signal });
    controller.abort();
    await assert.rejects(second, { name: 'AbortError' });
    unlock();
    await first;
    assert.equal(auth.authenticated, true);
});

test('runtime same-origin proxy and direct fallback retain password scope and reject redirects', async t => {
    const originalLocation = globalThis.location;
    const originalFetch = globalThis.fetch;
    t.after(() => { globalThis.location = originalLocation; globalThis.fetch = originalFetch; });
    globalThis.location = { href: 'https://chat.example/', origin: 'https://chat.example' };
    const wallet = new BrowserWalletRuntime();
    wallet.browserConfig = { deployment_api_proxy_path: '/zkapi-deployment/', trusted_deployment: funding };
    const calls = [];
    globalThis.fetch = async (url, init) => { calls.push({ url, init }); return json({}, 404); };
    wallet.testnetAuth = new TestnetPasswordAuth({ funding,
        fetch: (url, init) => wallet.remoteFetchRaw(url, init), requestPassword: null });
    const raw = wallet.testnetAuth.fetchRaw;
    wallet.testnetAuth.fetchRaw = async () => json({ authenticated: true });
    await wallet.testnetAuth.validate('shared-test-password');
    wallet.testnetAuth.fetchRaw = raw;
    await wallet.remoteFetch(`${server}/v2/requests`, { method: 'POST', body: 'proof' });
    assert.deepEqual(calls.map(call => call.url), ['https://chat.example/zkapi-deployment/v2/requests', `${server}/v2/requests`]);
    for (const call of calls) {
        assert.equal(new Headers(call.init.headers).get(TESTNET_PASSWORD_HEADER), 'shared-test-password');
        assert.equal(call.init.redirect, 'error');
        assert.equal(call.init.credentials, 'omit');
    }
});

test('funding and inference cannot reach wallet/proof work without Sepolia access', async t => {
    const previous = runtime.testnetAuth;
    t.after(() => { runtime.testnetAuth = previous; });
    const calls = [];
    runtime.testnetAuth = { ensure: async options => { calls.push(options); throw Object.assign(new Error('locked'), { code: 'testnet_password_required' }); } };
    const client = new ZkapiClient();
    client.initialized = true;
    await assert.rejects(client.prepareDepositQuote('1', { from: `0x${'11'.repeat(20)}` }), { code: 'testnet_password_required' });
    await assert.rejects(client.performDeposit('1'), { code: 'testnet_password_required' });
    await assert.rejects(client.acquireInferenceAccess('chat'), { code: 'testnet_password_required' });
    assert.equal(calls.length, 3);
    assert.equal(calls[1].interactive, true);
    assert.equal(calls[2].interactive, true);
});

test('cooperative withdrawal asks for access while unilateral escape keeps its independent path', async t => {
    const previous = runtime.testnetAuth;
    t.after(() => { runtime.testnetAuth = previous; });
    let prompts = 0;
    runtime.testnetAuth = { ensure: async () => { prompts++; throw Object.assign(new Error('locked'), { code: 'testnet_password_required' }); } };
    const client = new ZkapiClient();
    const withdrawals = [];
    client.performWithdrawal = async mode => { withdrawals.push(mode); return { mode }; };
    assert.deepEqual(await client.withdraw('escape'), { mode: 'escape' });
    assert.equal(prompts, 0);
    await assert.rejects(client.withdraw('mutual'), { code: 'testnet_password_required' });
    assert.deepEqual(withdrawals, ['escape']);
    assert.equal(prompts, 1);
});

for (const [originalNote, nextNote] of [[7, 8], [7, null], [null, 8]]) {
    test(`mutual withdrawal refuses a note changed from ${originalNote} to ${nextNote} during the password dialog`, async t => {
        const previousAuth = runtime.testnetAuth;
        t.after(() => { runtime.testnetAuth = previousAuth; });
        const client = new ZkapiClient();
        client.browserMode = true;
        client.wallet = { note: originalNote == null ? null : { note_id: originalNote } };
        runtime.testnetAuth = { async ensure() {
            client.wallet.note = nextNote == null ? null : { note_id: nextNote };
        } };
        t.mock.method(client, 'assertBalanceNotClaimed', async () => assert.fail('the changed note must never enter withdrawal'));
        await assert.rejects(client.withdraw('mutual', undefined, { destination: `0x${'12'.repeat(20)}` }),
            /private balance changed while withdrawal was opening/i);
        assert.equal(client.withdrawPromise, null);
    });
}
