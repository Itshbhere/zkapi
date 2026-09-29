export const TESTNET_PASSWORD_HEADER = 'X-ZKAPI-Testnet-Password';
const SEPOLIA_CHAIN_ID = 11155111;

function waitForAccess(operation, signal) {
    if (!signal) return operation;
    signal.throwIfAborted();
    return new Promise((resolve, reject) => {
        const aborted = () => reject(signal.reason || new DOMException('The operation was aborted.', 'AbortError'));
        signal.addEventListener('abort', aborted, { once: true });
        operation.then(resolve, reject).finally(() => signal.removeEventListener('abort', aborted));
    });
}

export function testnetPasswordRequired() {
    return Object.assign(new Error('Enter the Sepolia password to continue.'), {
        code: 'testnet_password_required', status: 401
    });
}

// This credential is shared by the test group, not an account identity. Keep it
// only in memory, outside wallet snapshots, persistence and proof payloads.
export class TestnetPasswordAuth {
    #password = null;
    #pending = null;
    constructor({ funding, fetch, requestPassword }) {
        this.enabled = Number(funding?.chain_id) === SEPOLIA_CHAIN_ID;
        this.base = this.enabled ? new URL(`${funding.protocol_server_url.replace(/\/+$/, '')}/`) : null;
        this.fetchRaw = fetch;
        this.requestPassword = requestPassword;
    }

    get authenticated() { return this.#password !== null; }
    clear() { this.#password = null; }

    protects(url) {
        if (!this.enabled) return false;
        const target = new URL(url, this.base);
        return target.origin === this.base.origin
            && target.pathname.startsWith(`${this.base.pathname}v2/`);
    }

    async validate(password, { signal } = {}) {
        if (!this.enabled) return;
        signal?.throwIfAborted();
        if (typeof password !== 'string' || password.length < 1 || password.length > 1024
            || !/^[\x20-\x7e]+$/.test(password) || password.trim() !== password) {
            throw testnetPasswordRequired();
        }
        const requestSignal = AbortSignal.any([AbortSignal.timeout(15_000), signal].filter(Boolean));
        const response = await this.fetchRaw(new URL('v2/auth', this.base).href, {
            method: 'GET', credentials: 'omit', cache: 'no-store', redirect: 'error', signal: requestSignal,
            headers: { [TESTNET_PASSWORD_HEADER]: password }
        });
        if (response.status === 401) {
            if (this.#password === password) this.clear();
            throw testnetPasswordRequired();
        }
        if (!response.ok) throw new Error('Unable to check Sepolia access. Try again shortly.');
        const result = await response.json();
        if (result.authenticated !== true) throw new Error('The Sepolia access response was invalid.');
        requestSignal.throwIfAborted();
        this.#password = password;
    }

    async ensure({ signal, interactive = false, changePassword = false } = {}) {
        if (!this.enabled) return;
        signal?.throwIfAborted();
        if (this.#pending) return waitForAccess(this.#pending, signal);
        // Background status/quote reads never own a dialog or prevent a later
        // explicit action from asking the user for the credential.
        if (!interactive) return this.#ensure({ signal, interactive, changePassword });
        const operation = this.#ensure({ signal, interactive, changePassword });
        this.#pending = operation;
        try { return await operation; }
        finally { if (this.#pending === operation) this.#pending = null; }
    }

    async #ensure({ signal, interactive, changePassword }) {
        if (changePassword) this.clear();
        if (this.#password) {
            try { await this.validate(this.#password, { signal }); return; }
            catch (error) { if (error.code !== 'testnet_password_required') throw error; }
        } else {
            const requestSignal = AbortSignal.any([AbortSignal.timeout(15_000), signal].filter(Boolean));
            const response = await this.fetchRaw(new URL('health', this.base).href, {
                credentials: 'omit', cache: 'no-store', redirect: 'error', signal: requestSignal
            });
            if (!response.ok) throw new Error('Unable to check Sepolia access. Try again shortly.');
            const status = await response.json();
            requestSignal.throwIfAborted();
            if (Number(status.chain_id) !== SEPOLIA_CHAIN_ID
                || typeof status.testnet_password_required !== 'boolean') {
                throw new Error('The Sepolia access configuration is unavailable.');
            }
            if (!status.testnet_password_required) return;
        }
        if (!interactive || typeof this.requestPassword !== 'function') throw testnetPasswordRequired();
        await this.requestPassword({ signal, authenticate: (password, { signal: attemptSignal } = {}) => this.validate(password, {
            signal: signal && attemptSignal ? AbortSignal.any([signal, attemptSignal]) : signal || attemptSignal
        }) });
        signal?.throwIfAborted();
        if (!this.#password) throw testnetPasswordRequired();
    }

    async fetch(url, init = {}) {
        const headers = new Headers(init.headers);
        headers.delete(TESTNET_PASSWORD_HEADER);
        const protectedRequest = this.protects(url);
        if (protectedRequest && !this.#password) await this.ensure({ signal: init.signal });
        const sentPassword = protectedRequest ? this.#password : null;
        if (sentPassword) headers.set(TESTNET_PASSWORD_HEADER, sentPassword);
        const response = await this.fetchRaw(url, {
            ...init, headers, credentials: 'omit',
            ...(protectedRequest ? { redirect: 'error', cache: 'no-store' } : {})
        });
        // Do not replay an operation after rejection: its caller owns recovery.
        if (protectedRequest && response.status === 401 && this.#password === sentPassword) this.clear();
        return response;
    }
}
