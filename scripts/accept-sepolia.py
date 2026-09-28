#!/usr/bin/env python3
"""Run an explicit Sepolia acceptance flow; preserves private wallets on failure.

Requires a native zkapi binary, cast, a funded encrypted keystore and password
file. Executes one tiny provider request and sends real Sepolia transactions.
Use --challenge to exercise historical-root escape evidence and the live daemon.
"""

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import socket
import subprocess
import time
import urllib.error
import urllib.request


ROOT = Path(__file__).resolve().parents[1]
WITHDRAWAL_ABI = "(uint16,uint64,address,uint256,uint256,uint256,uint256,uint256,uint32,uint128,address,uint256,bool,uint256)"


class CastCommandError(subprocess.CalledProcessError):
    def __str__(self):
        return f"cast {self.cmd[1]} failed (exit {self.returncode}): {self.stderr}"


def safe_cast_error(stderr, sensitive_values):
    message = stderr or "No error details returned"
    for value in sensitive_values:
        if value:
            message = message.replace(value, "[redacted]")
    message = re.sub(r"https?://[^\s\"'<>]+", "[RPC URL redacted]", message)
    message = re.sub(r"0x[0-9a-fA-F]{64,}", "[transaction data redacted]", message)
    return message.strip()[-3000:]


def number(value):
    return value if isinstance(value, int) else int(value, 16 if value.startswith("0x") else 10)


def address(value):
    return f"0x{number(value):040x}"


def http(url, body=None):
    request = urllib.request.Request(
        url,
        data=None if body is None else json.dumps(body).encode(),
        headers={"Content-Type": "application/json"} if body is not None else {},
    )
    with urllib.request.urlopen(request, timeout=180) as response:
        return json.load(response)


def wait_for(label, predicate, timeout=300):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            result = predicate()
            if result:
                return result
        except (OSError, ValueError, KeyError, subprocess.CalledProcessError):
            pass
        time.sleep(2)
    raise RuntimeError(f"Timed out waiting for {label}; keep the run directory for recovery")


class Acceptance:
    def __init__(self, args):
        self.args = args
        self.run = Path(args.run_dir).resolve()
        self.run.mkdir(mode=0o700, parents=True, exist_ok=False)
        self.manifest = http(args.deployment)
        m = self.manifest
        if number(m["chain_id"]) != 11155111 or m["protocol_version"] != 2:
            raise ValueError("Acceptance is restricted to Sepolia protocol v2")
        if m.get("proof_setup", {}).get("circuit_id") != "zkapi-v2-note-bound-v1":
            raise ValueError("Manifest does not identify the note-bound circuit")
        if not m.get("demo_mint_enabled") or m.get("deployment_status") == "migration_required":
            raise ValueError("Acceptance requires an enabled fresh demo-token deployment")
        self.vault = address(m["contract_address"])
        self.token = address(m["billing_token_address"])
        self.server = m["protocol_server_url"].rstrip("/")
        self.indexer = m["indexer_url"].rstrip("/")
        self.chain_env = dict(os.environ, ETH_RPC_URL=os.environ.get("ETH_RPC_URL") or m["rpc_url"])
        self.amount = number(m["request_charge_cap"]) * 2
        self.transactions = []
        self.process = None
        self.log = None
        self.client_url = None
        self.save("deployment.json", m)
        self.sender = self.cast("wallet", "address", *self.signer_args()).strip().lower()
        if number(self.cast("chain-id").strip()) != 11155111:
            raise ValueError("RPC is not Sepolia")

    def signer_args(self):
        return ["--keystore", self.args.keystore, "--password-file", self.args.password_file]

    def cast(self, *args):
        result = subprocess.run(
            ["cast", *map(str, args)], env=self.chain_env, check=False,
            capture_output=True, text=True, timeout=240,
        )
        if result.returncode:
            detail = safe_cast_error(result.stderr, [self.chain_env.get("ETH_RPC_URL"),
                                     self.args.keystore, self.args.password_file])
            raise CastCommandError(result.returncode, ["cast", str(args[0])], stderr=detail)
        return result.stdout

    def call(self, contract, signature, *args):
        return json.loads(self.cast("call", "--json", contract, signature, *args))

    def rpc(self, method, *params):
        try:
            return json.loads(self.cast("rpc", "--raw", method, json.dumps(params)))
        except CastCommandError as error:
            if re.search(r"\b(?:401|403)\b|forbidden|unauthorized", error.stderr or "", re.IGNORECASE):
                raise RuntimeError(f"RPC access denied during {method}: {error}") from error
            raise

    def send(self, label, contract, signature, *args):
        max_gas_price = getattr(self.args, "max_gas_price_wei", 3_000_000_000)
        wait_seconds = getattr(self.args, "wait_for_gas_seconds", 0)
        deadline = time.monotonic() + wait_seconds
        announced_wait = False
        while True:
            gas_price = getattr(self.args, "gas_price_wei", None)
            if gas_price is None:
                # A quoted legacy price can fall below base fee before the next
                # block. Add bounded headroom while keeping prefunding explicit.
                gas_price = (number(self.cast("gas-price").strip()) * 125 + 99) // 100
            if gas_price > max_gas_price:
                raise RuntimeError(f"Gas price {gas_price} wei exceeds the configured acceptance cap "
                                   f"{max_gas_price}; no {label} transaction sent")
            estimated_gas = number(self.cast("estimate", "--from", self.sender, contract, signature, *args).strip())
            gas_limit = (estimated_gas * 120 + 99) // 100
            if gas_limit > 16_777_216:
                raise RuntimeError(f"Padded gas limit {gas_limit} exceeds the Sepolia transaction limit")
            balance = number(self.cast("balance", self.sender).strip())
            required_balance = gas_limit * gas_price
            if balance >= required_balance:
                break
            if wait_seconds == 0:
                raise RuntimeError(f"Insufficient Sepolia ETH for {label}: balance {balance / 10**18:.8f}, "
                                   f"legacy maximum upfront fee {required_balance / 10**18:.8f} ETH "
                                   f"({gas_limit} gas at {gas_price} wei). No transaction sent.")
            if not announced_wait:
                print(f"Waiting for Sepolia gas funds for {label}: requires {required_balance / 10**18:.8f} ETH "
                      f"total; current balance {balance / 10**18:.8f} ETH. "
                      f"Polling balance every 10 seconds for up to {wait_seconds} seconds.", flush=True)
                announced_wait = True
            while balance < required_balance:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise RuntimeError(f"Timed out waiting for Sepolia gas funds for {label}. No transaction sent.")
                time.sleep(min(10, remaining))
                balance = number(self.cast("balance", self.sender).strip())
            # Funding can arrive after the old estimate/price is stale. Requote
            # only once funded, then check the new upfront bound before signing.
        self.save(label + "-gas.json", {"estimated_gas": estimated_gas, "gas_limit": gas_limit,
                                        "gas_price_wei": gas_price, "maximum_fee_wei": required_balance})
        nonce = number(self.cast("nonce", self.sender, "--block", "pending").strip())
        mined_nonce = number(self.cast("nonce", self.sender, "--block", "latest").strip())
        if nonce != mined_nonce:
            raise RuntimeError("Acceptance account already has an unresolved transaction; reconcile its "
                               "saved hash and nonce before starting another send. No transaction sent.")
        submission = {"status": "prepared", "sender": self.sender, "nonce": nonce,
                      "contract": contract, "signature": signature, "arguments": list(map(str, args)),
                      "gas_limit": gas_limit, "gas_price_wei": gas_price}
        self.save(label + "-submission.json", submission)
        transaction_hash = self.cast("send", "--async", "--legacy", "--gas-price", gas_price,
                                     "--gas-limit", gas_limit, "--nonce", nonce,
                                     *self.signer_args(), contract, signature, *args).strip()
        if not re.fullmatch(r"0x[0-9a-fA-F]{64}", transaction_hash):
            raise RuntimeError("cast async submission did not return a transaction hash; keep the saved "
                               f"{label}-submission.json nonce/call and reconcile before retrying")
        submission.update(status="submitted", transaction_hash=transaction_hash)
        self.save(label + "-submission.json", submission)
        self.transactions.append({"label": label, "hash": transaction_hash})
        print(f"{label} submitted (nonce {nonce}): {transaction_hash}", flush=True)
        def mined_receipt():
            return self.rpc("eth_getTransactionReceipt", transaction_hash)
        try:
            receipt = wait_for(f"receipt for {label}", mined_receipt,
                               timeout=getattr(self.args, "receipt_timeout_seconds", 900))
        except RuntimeError as error:
            raise RuntimeError(f"Transaction {transaction_hash} (nonce {nonce}, {label}) is still unresolved. "
                               f"Its submission record is saved; reconcile it before retrying. {error}") from error
        if receipt["transactionHash"].lower() != transaction_hash.lower():
            raise RuntimeError("RPC returned a receipt for another transaction; submission record preserved")
        self.save(label + "-receipt.json", receipt)
        submission["status"] = "confirmed" if number(receipt["status"]) == 1 else "reverted"
        self.save(label + "-submission.json", submission)
        if number(receipt["status"]) != 1:
            raise RuntimeError(f"Transaction {label} reverted")
        print(f"{label} confirmed: {transaction_hash}", flush=True)
        return receipt

    def save(self, name, data):
        temporary = self.run / (name + ".tmp")
        with temporary.open("w") as stream:
            stream.write(json.dumps(data, indent=2) + "\n")
            stream.flush()
            os.fsync(stream.fileno())
        temporary.replace(self.run / name)

    def local(self, path, body=None):
        return http(self.client_url + path, body)

    def stop_client(self):
        if self.process is not None:
            self.process.terminate()
            try:
                self.process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
            self.process = None
        if self.log is not None:
            self.log.close()
            self.log = None

    def start_client(self, name):
        self.stop_client()
        # Reserve an unused loopback port immediately before launching the daemon.
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        self.client_url = f"http://127.0.0.1:{port}"
        self.log = (self.run / f"{name}-client.log").open("a")
        self.process = subprocess.Popen(
            [self.args.zkapi, "client", "--deployment", str(self.run / "deployment.json"),
             "--state-dir", str(self.run / name), "--listen", f"127.0.0.1:{port}", "--no-fund"],
            cwd=ROOT, stdout=self.log, stderr=subprocess.STDOUT,
        )
        def ready():
            if self.process.poll() is not None:
                raise RuntimeError(f"Native client exited; inspect {name}-client.log")
            return self.local("/health")
        wait_for("native client startup", ready)

    def synced(self):
        chain_root = number(self.call(self.vault, "currentRoot()(uint256)")[0])
        return (number(http(self.indexer + "/v1/tree/root")["root"]) == chain_root
                and number(http(self.server + "/health")["current_root"]) == chain_root)

    def note(self, note_id):
        return self.call(self.vault, "notes(uint32)(bytes32,uint128,uint64,uint8)", note_id)

    def preflight(self):
        health = http(self.server + "/health")
        attestation = http(self.server + "/v1/attestation")
        for response in [health, attestation]:
            assert number(response["chain_id"]) == 11155111
            assert response["protocol_version"] == 2
            assert address(response["contract_address"]) == self.vault
        assert address(self.call(self.vault, "billingToken()(address)")[0]) == self.token
        assert number(self.call(self.token, "decimals()(uint8)")[0]) == 6
        assert number(self.call(self.vault, "requestChargeCap()(uint128)")[0]) == self.amount // 2
        for key, prefix in [("state_signing_key", "stateSigningKey"),
                            ("clearance_signing_key", "clearanceSigningKey")]:
            for axis in ["x", "y"]:
                onchain = self.call(self.vault, prefix + axis.upper() + "()(uint256)")[0]
                assert number(onchain) == number(self.manifest[key][axis]) == number(attestation[key][axis])
        for name in ["request.pk", "withdrawal.pk"]:
            with urllib.request.urlopen(self.server + "/proofs/" + name, timeout=120) as response:
                public_hash = hashlib.sha256(response.read()).hexdigest()
            local_hash = hashlib.sha256((ROOT / "protocol/setup/v2" / name).read_bytes()).hexdigest()
            assert public_hash == local_hash, f"Public {name} differs from the native proof setup"
        for path in ["/v1/dashboard/recent", "/v1/dashboard/events"]:
            try:
                http(self.server + path)
            except urllib.error.HTTPError as error:
                assert error.code == 404
            else:
                raise AssertionError("Operator dashboard is publicly exposed")
        wait_for("chain/indexer/server root agreement", self.synced)
        self.save("public-health.json", health)
        print("Public deployment bindings, proving artifacts, private routes and roots verified", flush=True)

    def deposit(self, name):
        self.start_client(name)
        plan = self.local("/deposit/prepare", {"amount": self.amount})
        self.save(name + "-deposit-private.json", plan)
        note_id = plan["next_note_id"]
        self.send(name + "-deposit", self.vault, "deposit(bytes32,uint128,uint256[32])",
                  f"0x{number(plan['commitment']):064x}", self.amount,
                  "[" + ",".join(plan["zero_path"]) + "]")
        wait_for("deposited note indexing", lambda: self.synced()
                 and number(http(self.indexer + "/v1/tree/next-note-id")["next_note_id"]) > note_id)
        note = self.note(note_id)
        assert number(note[1]) == self.amount and number(note[3]) == 1
        self.local("/deposit/confirm", {"secret": plan["secret"], "note_id": note_id,
                                      "amount": self.amount, "expiry_ts": number(note[2])})
        return note_id

    def withdrawal(self, mode):
        plan = self.local("/wallet/withdraw", {"mode": mode, "destination": self.sender})
        self.save(f"{mode}-{plan['public_inputs']['note_id']}-plan-private.json", plan)
        p = plan["public_inputs"]
        assert address(p["contract_address"]) == self.vault and number(p["chain_id"]) == 11155111
        assert p["protocol_version"] == 2 and p["has_clearance"] == (mode == "mutual")
        destination = p["destination"]
        if isinstance(destination, list):
            destination = "0x" + bytes(destination).hex()
        assert destination.lower() == self.sender
        values = [p[k] for k in ["protocol_version", "chain_id", "contract_address", "active_root",
                  "state_signing_key_x", "state_signing_key_y", "clearance_signing_key_x",
                  "clearance_signing_key_y", "note_id", "final_balance"]]
        values[2] = self.vault
        values += [destination, p["withdrawal_nullifier"], str(p["has_clearance"]).lower(), p["withdrawal_tag"]]
        proof = base64.b64decode(plan["proof"]["proof"], validate=True)
        assert len(proof) == 256 and len(plan["siblings"]) == 32
        function = "mutualClose" if mode == "mutual" else "initiateEscapeWithdrawal"
        receipt = self.send(f"{mode}-{p['note_id']}", self.vault,
                            function + "(" + WITHDRAWAL_ABI + ",bytes,uint256[32])",
                            "(" + ",".join(map(str, values)) + ")", "0x" + proof.hex(),
                            "[" + ",".join(plan["siblings"]) + "]")
        return plan, receipt

    def historical_challenge(self, note_a, request_root):
        self.stop_client()
        note_b = self.deposit("wallet-b")
        assert number(http(self.indexer + "/v1/tree/root")["root"]) != number(request_root)
        self.start_client("stale-wallet-a")
        plan, receipt = self.withdrawal("escape")
        assert plan["public_inputs"]["note_id"] == note_a
        assert number(plan["public_inputs"]["active_root"]) != number(request_root)
        topic = self.cast("keccak", "EscapeWithdrawalChallenged(uint32,uint256,uint256)").strip()
        query = {"address": self.vault, "fromBlock": hex(number(receipt["blockNumber"])), "toBlock": "latest",
                 "topics": [topic, f"0x{note_a:064x}"]}
        def challenged():
            logs = self.rpc("eth_getLogs", query)
            if not logs or number(self.note(note_a)[3]) != 1:
                return False
            log = logs[-1]
            if number(self.cast("block-number").strip()) < number(log["blockNumber"]) + 2:
                return False
            return log
        log = wait_for("confirmed live historical-root escape challenge", challenged, timeout=600)
        self.save("escape-challenge-event.json", log)
        self.transactions.append({"label": "live-escape-challenge", "hash": log["transactionHash"]})
        pending = self.call(self.vault, "pendingWithdrawals(uint32)(bool,uint256,uint256,uint128,address,uint64)", note_a)
        assert pending[0] is False or str(pending[0]).lower() == "false"
        wait_for("restored note indexing", self.synced)
        print(f"Historical-root escape challenged: {log['transactionHash']}", flush=True)
        self.start_client("wallet-b")
        self.close(note_b, self.amount)

    def close(self, note_id, expected_balance):
        before = number(self.call(self.token, "balanceOf(address)(uint256)", self.sender)[0])
        plan, _ = self.withdrawal("mutual")
        assert plan["public_inputs"]["note_id"] == note_id
        assert number(plan["public_inputs"]["final_balance"]) == expected_balance
        assert number(self.note(note_id)[3]) == 3
        after = number(self.call(self.token, "balanceOf(address)(uint256)", self.sender)[0])
        assert after - before == expected_balance
        status = self.local("/wallet/withdraw/confirm", {})
        assert status["status"] == "closed" and not status["wallet"]["has_note"]
        wait_for("closed note indexing", self.synced)

    def execute(self):
        self.preflight()
        treasury = address(self.call(self.vault, "treasury()(address)")[0])
        if treasury == self.sender:
            raise ValueError("Acceptance depositor must differ from the operator treasury")
        treasury_before = number(self.call(self.token, "balanceOf(address)(uint256)", treasury)[0])
        total = self.amount * (2 if self.args.challenge else 1)
        self.send("faucet-mint", self.token, "mint(address,uint256)", self.sender, total)
        self.send("approve", self.token, "approve(address,uint256)", self.vault, total)
        note_a = self.deposit("wallet-a")
        if self.args.challenge:
            self.stop_client()
            shutil.copytree(self.run / "wallet-a", self.run / "stale-wallet-a")
            self.start_client("wallet-a")
        request_root = http(self.indexer + "/v1/tree/root")["root"]
        response = self.local("/request", {"method": "POST", "path": "/v1/chat/completions", "headers": {},
                              "body": {"model": self.args.model, "max_tokens": 8,
                                       "messages": [{"role": "user", "content": "Reply with OK."}]}})
        self.save("metered-response.json", response)
        charge = number(response["charge_applied"])
        assert response["response_code"] == 200 and 0 <= charge <= self.amount // 2
        assert number(response["remaining_balance"]) == self.amount - charge
        assert self.local("/wallet/status")["note"]["is_genesis"] is False
        print(f"Native Groth16 request accepted; charged {charge} demo credit units", flush=True)
        if self.args.challenge:
            self.historical_challenge(note_a, request_root)
            self.start_client("wallet-a")
        self.close(note_a, self.amount - charge)
        treasury_after = number(self.call(self.token, "balanceOf(address)(uint256)", treasury)[0])
        assert treasury_after - treasury_before == charge
        summary = {"deployment": self.manifest["deployment_id"], "status": "passed",
                   "chain_id": 11155111, "note_id": note_a, "charge": charge,
                   "historical_root_live_challenge": self.args.challenge, "transactions": self.transactions}
        self.save("summary.json", summary)
        print(json.dumps(summary, indent=2), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--deployment", required=True, help="Public HTTPS config.json URL")
    parser.add_argument("--zkapi", required=True, help="Path to the native release zkapi binary")
    parser.add_argument("--keystore", required=True, help="Funded dedicated acceptance account's encrypted keystore")
    parser.add_argument("--password-file", required=True, help="Private keystore password file")
    parser.add_argument("--run-dir", required=True, help="New private directory; must not already exist")
    parser.add_argument("--model", required=True, help="Configured affordable model, e.g. an OpenRouter org/model ID")
    parser.add_argument("--challenge", action="store_true", help="Also exercise the live historical-root escape challenge")
    parser.add_argument("--gas-price-wei", type=int, help="Explicit legacy gas price; otherwise current RPC gas price plus 25%% headroom")
    parser.add_argument("--max-gas-price-wei", type=int, default=3_000_000_000,
                        help="Abort above this gas price (default: 3 gwei)")
    parser.add_argument("--wait-for-gas-seconds", type=int, default=0,
                        help="Wait for Sepolia ETH top-ups before each send (default: fail immediately)")
    parser.add_argument("--receipt-timeout-seconds", type=int, default=900,
                        help="Wait for each asynchronously submitted transaction receipt (default: 900 seconds)")
    args = parser.parse_args()
    if not args.deployment.startswith("https://"):
        parser.error("--deployment must be the public HTTPS endpoint")
    if args.max_gas_price_wei <= 0 or (args.gas_price_wei is not None and args.gas_price_wei <= 0):
        parser.error("Gas prices must be positive integer wei values")
    if args.wait_for_gas_seconds < 0:
        parser.error("--wait-for-gas-seconds must be nonnegative")
    if args.receipt_timeout_seconds <= 0:
        parser.error("--receipt-timeout-seconds must be positive")
    args.zkapi = str(Path(args.zkapi).resolve())
    args.keystore = str(Path(args.keystore).resolve())
    args.password_file = str(Path(args.password_file).resolve())
    os.umask(0o077)
    acceptance = None
    try:
        acceptance = Acceptance(args)
        acceptance.execute()
    finally:
        if acceptance is not None:
            acceptance.stop_client()


if __name__ == "__main__":
    main()
