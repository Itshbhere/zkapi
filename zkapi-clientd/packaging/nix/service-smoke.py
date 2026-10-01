#!/usr/bin/env python3
"""Run generated user units in the disposable Nix Docker test container only."""

import os
from pathlib import Path
import shutil
import subprocess
import sys
import time


def run(*args, check=True):
    result = subprocess.run(args, text=True, capture_output=True, timeout=30)
    if check and result.returncode != 0:
        raise RuntimeError(f"{' '.join(args)} failed ({result.returncode}):\n{result.stdout}{result.stderr}")
    return result


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def wait_for(predicate, message):
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.1)
    raise RuntimeError(message)


def main():
    require(os.getuid() == 1000 and Path.home() == Path("/home/nix-test"),
            "This test is only for the disposable Docker test user.")
    source = Path(sys.argv[1])
    units = Path.home() / ".config/systemd/user"
    units.mkdir(parents=True)
    state = Path.home() / "private state/zkapi-clientd"
    log = Path.home() / "service.log"
    dropin = units / "zkapi-clientd.service.d"
    dropin.mkdir()
    # Capture the real daemon's readiness error without needing journald. All
    # execution, conditions, restart behavior and hardening remain unchanged.
    (dropin / "test-log.conf").write_text(
        f"[Service]\nStandardOutput=append:{log}\nStandardError=append:{log}\n")
    # Nix keeps upstream units under example/; both the manager and analyzer
    # need that search path to resolve basic.target and default.target.
    os.environ["SYSTEMD_UNIT_PATH"] = f"{units}:/opt/tools/example/systemd/user"
    with (Path.home() / "manager.log").open("w") as manager_log:
        manager = subprocess.Popen(["/opt/tools/lib/systemd/systemd", "--user"],
                                   stdout=manager_log, stderr=manager_log)
        try:
            wait_for(lambda: run("systemctl", "--user", "show-environment", check=False).returncode == 0,
                     "The isolated systemd user manager did not become ready.")
            for module in ("nixos", "home-manager"):
                unit = units / "zkapi-clientd.service"
                shutil.copyfile(source / module / unit.name, unit)
                run("systemd-analyze", "--user", "verify", str(unit))
                run("systemctl", "--user", "daemon-reload")
                run("systemctl", "--user", "start", unit.name)
                condition = run("systemctl", "--user", "show", unit.name,
                                "--property=ConditionResult", "--value").stdout.strip()
                require(condition == "no", f"{module} started without an initialized profile.")
                require(not state.exists(), f"{module} created private state without consent.")

                state.mkdir(parents=True, mode=0o700)
                profile = state / "config.json"
                profile.write_text("{}\n")
                profile.chmod(0o600)
                log.unlink(missing_ok=True)
                run("systemctl", "--user", "start", unit.name)
                wait_for(lambda: log.exists() and "Run zkapi-clientd config" in log.read_text(),
                         f"{module} did not execute the packaged daemon and reject the invalid profile.")
                wait_for(lambda: run("systemctl", "--user", "show", unit.name,
                                     "--property=ExecMainStatus", "--value").stdout.strip() == "1",
                         f"{module} did not fail readiness with exit status 1.")
                run("systemctl", "--user", "stop", unit.name)
                # Inactive units can already have been garbage-collected.
                run("systemctl", "--user", "reset-failed")
                require(profile.read_text() == "{}\n", f"{module} changed the saved profile.")
                require(list(state.iterdir()) == [profile], f"{module} created wallet state.")
                shutil.rmtree(state)
                print(f"{module}: actual user service skipped missing profile, executed daemon, "
                      "rejected invalid profile, stopped and preserved state.", flush=True)
        finally:
            run("systemctl", "--user", "stop", "zkapi-clientd.service", check=False)
            run("systemctl", "--user", "exit", check=False)
            try:
                manager.wait(timeout=10)
            except subprocess.TimeoutExpired:
                manager.kill()
                manager.wait(timeout=10)


if __name__ == "__main__":
    main()
