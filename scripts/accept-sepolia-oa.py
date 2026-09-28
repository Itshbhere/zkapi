#!/usr/bin/env python3
"""Verify OA-org issuance, pinned verification, inference, settlement and refund.

Explicitly sends Sepolia transactions and one paid eight-token completion when
run. Reuses accept-sepolia.py without changing its proxy/challenge acceptance.
The private run directory is retained for recovery if any stage fails.
"""

import argparse
import importlib.util
import json
import os
from pathlib import Path
import socket
import subprocess
import time
import urllib.error


spec = importlib.util.spec_from_file_location("sepolia_acceptance", Path(__file__).with_name("accept-sepolia.py"))
base = importlib.util.module_from_spec(spec)
spec.loader.exec_module(base)


class OaAcceptance(base.Acceptance):
    def start_client(self, name):
        self.stop_client()
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        self.client_url = f"http://127.0.0.1:{port}"
        self.log_path = self.run / f"{name}-client.log"
        self.log = self.log_path.open("a")
        self.process = subprocess.Popen(
            [self.args.zkapi, "--require-oa-org-key-source",
             "--oa-verifier-url", self.args.verifier_url,
             "--openrouter-inference-base", "https://openrouter.ai/api/v1",
             "client", "--mode", "direct-openrouter",
             "--deployment", str(self.run / "deployment.json"),
             "--state-dir", str(self.run / name), "--listen", f"127.0.0.1:{port}", "--no-fund"],
            cwd=base.ROOT, stdout=self.log, stderr=subprocess.STDOUT,
            env=dict(os.environ, RUST_LOG="info"),
        )

        def ready():
            if self.process.poll() is not None:
                raise RuntimeError(f"Native OA client exited; inspect {name}-client.log")
            return self.local("/health")

        base.wait_for("native OA client startup", ready)
        config = self.local("/zkapi/v1/config")
        assert config["request_mode"] == "direct_openrouter"
        # Older native clients derive the UI hint from the optional private
        # dashboard. Public health is the same capability source enforced by
        # native startup; actual lease issuance is checked below.
        assert "direct_openrouter" in base.http(self.server + "/health")["request_modes"]

    def finish_lease(self, lease):
        requested_at = int(time.time())
        if self.args.settlement == "expiry":
            print(f"Waiting for lease expiry/settlement at {lease['settle_after']}", flush=True)

            def settled():
                status = self.local("/wallet/status")
                return status if not status["pending_request"] else False

        else:
            print("Requesting authenticated early OA lease retirement", flush=True)

            def settled():
                try:
                    status = self.local("/wallet/settle", {})
                except urllib.error.HTTPError as error:
                    if error.code not in (409, 502):
                        raise RuntimeError(f"Lease settlement returned HTTP {error.code}") from error
                    return False
                return status if not status["pending_request"] else False

        status = base.wait_for("OA signed usage receipt and native wallet recovery", settled,
                               timeout=self.args.settlement_timeout)
        assert status["has_note"] and status["note"]["is_genesis"] is False
        assert not self.local("/zkapi/v1/config").get("active_lease")
        return status, requested_at

    def execute(self):
        self.preflight()
        health = base.http(self.server + "/health")
        assert "direct_openrouter" in health["request_modes"]
        treasury = base.address(self.call(self.vault, "treasury()(address)")[0])
        if treasury == self.sender:
            raise ValueError("Acceptance depositor must differ from the operator treasury")
        treasury_before = base.number(self.call(self.token, "balanceOf(address)(uint256)", treasury)[0])
        self.send("oa-faucet-mint", self.token, "mint(address,uint256)", self.sender, self.amount)
        self.send("oa-approve", self.token, "approve(address,uint256)", self.vault, self.amount)
        note_id = self.deposit("wallet-oa")
        response = self.local("/request", {
            "method": "POST", "path": "/v1/chat/completions", "headers": {},
            "body": {"model": self.args.model, "max_tokens": 8, "stream": False,
                     "messages": [{"role": "user", "content": "Reply with OK."}]},
        })
        self.save("oa-completion.json", response)
        assert response["response_code"] == 200
        assert base.number(response["charge_applied"]) == 0, "Direct usage must settle before charging"
        assert base.number(response["remaining_balance"]) == self.amount
        payload = response["payload"]
        assert payload["choices"][0]["message"]["content"], "Provider returned no completion text"
        assert 0 < base.number(payload["usage"]["completion_tokens"]) <= 8

        # --require-oa-org-key-source forbids fallback to an ordinary direct key.
        # This success log occurs only after the pinned verifier returns verified.
        verifier_lines = [line for line in self.log_path.read_text().splitlines()
                          if "OA verifier accepted station-issued OpenRouter key" in line]
        assert verifier_lines, "Pinned OA verifier success was not observed"
        self.save("oa-verification-evidence.json", {"verifier_url": self.args.verifier_url,
                  "required_key_source": "oa_org", "native_success_log": verifier_lines})
        request_id = response["client_request_id"]
        active = self.local("/zkapi/v1/config")["active_lease"]
        assert active["client_request_id"] == request_id
        assert self.local("/wallet/status")["pending_request"] is True
        lease_url = self.server + "/v2/openrouter/leases/" + request_id
        lease = base.http(lease_url)
        assert lease["status"] == "active" and lease["expires_at"] > int(time.time())
        assert lease["spending_limit_usd"] <= (self.amount // 2) / 1_000_000
        self.save("oa-issued-lease.json", lease)
        print("OA lease verified by pinned verifier; eight-token direct inference succeeded", flush=True)

        status, requested_at = self.finish_lease(lease)
        settled = base.http(lease_url)
        assert settled["status"] == "finalized"
        recovery = base.http(self.server + "/v2/requests/" + request_id)
        finalized = recovery["request_response"]
        receipt = json.loads(finalized["response_payload"])
        assert receipt["type"] == "oa_org_ephemeral_lease_settlement"
        charge = base.number(finalized["charge_applied"])
        assert 0 < charge <= self.amount // 2, "Use a paid model to verify a nonzero measured charge"
        assert base.number(settled["charge_applied"]) == base.number(receipt["usage_credits"]) == charge
        assert lease["issued_at"] <= receipt["usage_receipt_closed_at"] <= receipt["usage_receipt_expires_at"]
        assert receipt["usage_finalized_at"] >= receipt["usage_receipt_closed_at"]
        if self.args.settlement == "explicit":
            assert requested_at < lease["expires_at"], "Retirement was not requested before lease expiry"
            assert receipt["usage_receipt_closed_at"] < receipt["usage_receipt_expires_at"], \
                "Explicit retirement did not close the provider key before expiry"
        for field in ("station_id", "station_signature", "org_signature"):
            assert receipt[field], f"OA final usage receipt omitted {field}"
        assert base.number(status["note"]["current_balance"]) == self.amount - charge
        self.save("oa-finalized-lease.json", settled)
        self.save("oa-final-usage-receipt.json", receipt)
        self.save("oa-recovered-wallet.json", status)

        self.close(note_id, self.amount - charge)
        treasury_after = base.number(self.call(self.token, "balanceOf(address)(uint256)", treasury)[0])
        assert treasury_after - treasury_before == charge
        summary = {"deployment": self.manifest["deployment_id"], "status": "passed",
                   "chain_id": 11155111, "mode": "direct_openrouter", "key_source": "oa_org",
                   "verifier_url": self.args.verifier_url, "note_id": note_id,
                   "client_request_id": request_id, "completion_tokens": payload["usage"]["completion_tokens"],
                   "charge": charge, "settlement": self.args.settlement,
                   "retirement_requested_at": requested_at if self.args.settlement == "explicit" else None,
                   "closed_before_provider_expiry": receipt["usage_receipt_closed_at"] < receipt["usage_receipt_expires_at"],
                   "transactions": self.transactions}
        self.save("summary.json", summary)
        print(json.dumps(summary, indent=2), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for option in ("deployment", "zkapi", "keystore", "password-file", "run-dir", "model"):
        parser.add_argument("--" + option, required=True)
    parser.add_argument("--verifier-url", default="https://verifier2.openanonymity.ai",
                        help="Independent verifier trust anchor; never derive it from a lease response")
    parser.add_argument("--settlement", choices=("explicit", "expiry"), default="explicit")
    parser.add_argument("--settlement-timeout", type=int, default=600)
    parser.add_argument("--gas-price-wei", type=int, help="Optional explicit Sepolia legacy gas price")
    parser.add_argument("--max-gas-price-wei", type=int, default=3_000_000_000,
                        help="Reject acceptance transactions above this gas price")
    args = parser.parse_args()
    if not args.deployment.startswith("https://") or not args.verifier_url.startswith("https://"):
        parser.error("Deployment and verifier must use HTTPS")
    if not 60 <= args.settlement_timeout <= 1800:
        parser.error("Settlement timeout must be between 60 and 1800 seconds")
    for field in ("zkapi", "keystore", "password_file"):
        setattr(args, field, str(Path(getattr(args, field)).resolve()))
    os.umask(0o077)
    acceptance = None
    try:
        acceptance = OaAcceptance(args)
        acceptance.execute()
    finally:
        if acceptance is not None:
            acceptance.stop_client()


if __name__ == "__main__":
    main()
