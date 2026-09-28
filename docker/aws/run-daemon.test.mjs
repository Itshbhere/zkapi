import assert from 'node:assert/strict';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { test } from 'node:test';

// Exercise the real launcher with the documented environment and harmless
// executables. This catches shell startup failures without RPCs or credentials.
for (const role of ['server', 'challenger']) {
    for (const override of [null, '/custom/proofs']) {
        test(`${role} launches with ${override ? 'explicit' : 'default'} proof artifacts`, async () => {
            const bin = await mkdtemp(join(tmpdir(), 'zkapi-launcher-'));
            try {
                for (const name of ['zkapi', 'zkapi-challenged']) {
                    await writeFile(join(bin, name), `#!/bin/sh\nprintf '%s\\n' "$@"\n`, { mode: 0o755 });
                }
                const env = {
                    PATH: `${bin}:/usr/bin:/bin`,
                    CHAIN_ID: '11155111',
                    VAULT_ADDRESS: '0x0000000000000000000000000000000000000001',
                    DEPLOY_BLOCK: '1',
                    CIRCUIT_ID: 'zkapi-v2-note-bound-v1',
                    REQUEST_CHARGE_CAP: '1000000',
                    ZKAPI_STATE_SEED: 'test-state-seed',
                    ZKAPI_CLEAR_SEED: 'test-clear-seed',
                    ZKAPI_CHALLENGE_RPC_URL: 'http://signer:8547',
                    ZKAPI_CHALLENGE_SENDER: '0x0000000000000000000000000000000000000002',
                };
                if (override) env.ZKAPI_PROOF_SETUP_DIR = override;
                const result = spawnSync('/bin/sh', [resolve('docker/aws/run-daemon.sh'), role], {
                    env, encoding: 'utf8', timeout: 5000,
                });
                assert.equal(result.status, 0, result.stderr);
                const args = result.stdout.trim().split('\n');
                const index = args.indexOf('--proof-setup-dir');
                assert.ok(index >= 0);
                assert.equal(args[index + 1], override || '/srv/zkapi/protocol/setup/v2');
                assert.ok(!result.stdout.includes('test-state-seed'));
                assert.ok(!result.stdout.includes('test-clear-seed'));
            } finally {
                await rm(bin, { recursive: true, force: true });
            }
        });
    }
}
