#!/usr/bin/env python3
"""Run package/service smoke tests in disposable Docker containers over SSH.

Examples (from any working directory):
  python3 zkapi-clientd/scripts/test-platforms-ssh.py user@docker-host.example
  python3 zkapi-clientd/scripts/test-platforms-ssh.py user@docker-host.example --platform arch

Requires local ssh/rsync and remote Python 3, curl, Docker, and Bash. The Docker
runners start isolated systemd managers; some require privileged containers.
No host wallet directory is copied or mounted. This checks package installation
and service readiness rejection, not funded inference or macOS launchd.
"""
import argparse
import json
from pathlib import Path
import re
import shlex
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[2]


def remote(host, argv, **kwargs):
    return subprocess.run(["ssh", "-o", "BatchMode=yes", host, shlex.join(map(str, argv))],
                          check=True, **kwargs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("host", help="Explicit SSH host alias or user@host for Docker tests")
    parser.add_argument("--platform", choices=("all", "nix", "arch", "homebrew"), default="all")
    parser.add_argument("--artifacts", type=Path, help="Copy cached native archives instead of downloading them remotely")
    parser.add_argument("--logs", type=Path, default=ROOT / "zkapi-clientd/build/platform-tests")
    parser.add_argument("--keep", action="store_true", help="Retain this run's remote source/archive directory")
    args = parser.parse_args()
    if args.host.startswith("-") or not re.fullmatch(r"[A-Za-z0-9_.@:-]+", args.host):
        parser.error("host must be an SSH host alias or user@host")
    formula = (ROOT / "zkapi-clientd/packaging/homebrew/zkapi-clientd.rb").read_text()
    version = re.search(r'^  version "([0-9]+\.[0-9]+\.[0-9]+)"$', formula, re.M).group(1)
    subprocess.run([sys.executable, str(ROOT / "zkapi-clientd/scripts/sync-packages.py"),
                    version, "--check"], check=True)
    record = ROOT / f"zkapi-clientd/packaging/releases/clientd-{version}.json"
    # Fail locally before allocating remote resources if checksum metadata is absent.
    json.loads(record.read_text())
    args.logs.mkdir(parents=True, exist_ok=True)
    stage = remote(args.host, ["mktemp", "-d", "/tmp/zkapi-platform-tests.XXXXXXXX"],
                   capture_output=True, text=True).stdout.strip()
    if not re.fullmatch(r"/tmp/zkapi-platform-tests\.[A-Za-z0-9]+", stage):
        raise RuntimeError(f"Unexpected remote temporary directory: {stage!r}")
    print(f"Remote workspace: {args.host}:{stage}", flush=True)
    try:
        remote(args.host, ["mkdir", "-p", f"{stage}/src", f"{stage}/release"])
        subprocess.run(["rsync", "-az", "--exclude=build", "--exclude=dist", "--exclude=__pycache__",
                        "--exclude=.DS_Store", str(ROOT / "zkapi-clientd"),
                        f"{args.host}:{stage}/src/"], check=True)
        if args.artifacts:
            archives = [args.artifacts.resolve() / f"zkapi-clientd_{version}_{target}.tar.gz"
                        for target in ("linux_amd64", "linux_arm64", "darwin_amd64", "darwin_arm64")]
            for archive in archives:
                if not archive.is_file():
                    raise FileNotFoundError(archive)
            subprocess.run(["rsync", "-az", *map(str, archives), f"{args.host}:{stage}/release/"], check=True)
        # Only public native archives are fetched. Hashes are checked against the
        # pinned metadata before any runner may execute a packaged binary.
        prepare = r'''
import hashlib,json,pathlib,subprocess,sys
stage=pathlib.Path(sys.argv[1]); version=sys.argv[2]
record=json.loads((stage/'src/zkapi-clientd/packaging/releases'/f'clientd-{version}.json').read_text())
for target in ('linux_amd64','linux_arm64','darwin_amd64','darwin_arm64'):
    name=f'zkapi-clientd_{version}_{target}.tar.gz'; path=stage/'release'/name
    if not path.exists():
        subprocess.run(['curl','--fail','--location','--silent','--show-error','--retry','3',
            f'https://github.com/ethereum/zkapi/releases/download/clientd-v{version}/{name}',
            '--output',str(path)],check=True)
    if hashlib.sha256(path.read_bytes()).hexdigest()!=record['artifacts'][name]:
        raise RuntimeError(f'Release checksum mismatch: {name}')
subprocess.run(['python3',str(stage/'src/zkapi-clientd/scripts/assemble-release.py'),
    version,str(stage/'release')],check=True)
'''
        remote(args.host, ["python3", "-c", prepare, stage, version])
        platforms = ("nix", "arch", "homebrew") if args.platform == "all" else (args.platform,)
        for platform in platforms:
            runner = f"{stage}/src/zkapi-clientd/packaging/{platform}/test-docker.sh"
            command = ["bash", runner]
            if platform == "nix":
                command.append(f"{stage}/src")
            command.append(f"{stage}/release")
            log = args.logs / f"{platform}.log"
            print(f"Testing {platform}; log: {log}", flush=True)
            with log.open("w") as output:
                process = subprocess.Popen(["ssh", "-o", "BatchMode=yes", args.host, shlex.join(command)],
                                           stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
                for line in process.stdout:
                    output.write(line)
                    output.flush()
                    print(line, end="", flush=True)
                if process.wait() != 0:
                    raise RuntimeError(f"{platform} Docker check failed; see {log}")
        print("All requested Docker package/service checks passed.")
    finally:
        if args.keep:
            print(f"Retained {args.host}:{stage}")
        else:
            # Only this invocation's mktemp directory, never a user's checkout.
            remote(args.host, ["rm", "-rf", "--", stage])


if __name__ == "__main__":
    main()
