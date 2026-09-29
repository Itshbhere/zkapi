import assert from 'node:assert/strict';
import test from 'node:test';
import { ZkapiClient } from '../services/zkapiClient.js';

test('initialization starts the browser wallet without probing a local daemon or honoring its old URL override', async t => {
    const oldWindow = globalThis.window;
    t.after(() => { globalThis.window = oldWindow; });
    globalThis.window = { location: { search: '?zkapiMode=daemon' }, setInterval: () => 1 };
    t.mock.method(globalThis, 'fetch', async () => assert.fail('no same-origin daemon probe'));
    const client = new ZkapiClient();
    const calls = [];
    t.mock.method(client, 'enableBrowserMode', async () => { calls.push('browser'); });
    t.mock.method(client, 'refresh', async () => { calls.push('refresh'); });
    t.mock.method(client, 'reconcileBrowserWithdrawalsOnLoad', async () => { calls.push('recovery'); });
    t.mock.method(client, 'attachWalletEvents', () => {});
    await client.init();
    assert.deepEqual(calls, ['browser', 'refresh', 'recovery']);
    assert.equal(client.browserMode, true);
    assert.equal(client.requestMode, 'direct_openrouter');
});
