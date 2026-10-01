#!/usr/bin/env bash
# Run on the Docker host. Both arguments are paths on that host.
set -euo pipefail
if [[ $# != 2 ]]; then
  echo "Usage: $0 REPOSITORY_ROOT RELEASE_ARTIFACTS" >&2
  exit 2
fi
repo=$(cd "$1" && pwd)
artifacts=$(cd "$2" && pwd)
test -f "$repo/zkapi-clientd/scripts/test-nix.py"
test -f "$artifacts/nix/package.nix"
container="zkapi-nix-check-$$"
cleanup() { docker rm -f "$container" >/dev/null 2>&1 || true; }
trap cleanup EXIT
# systemd needs writable cgroups. The private namespace exposes only this
# container's cgroup; no host cgroup tree or Docker socket is mounted.
docker run -d --name "$container" --privileged --cgroupns=private \
  --mount "type=bind,src=$repo,dst=/repo,readonly" \
  --mount "type=bind,src=$artifacts,dst=/release,readonly" \
  nixos/nix:2.24.14 sleep infinity >/dev/null
docker exec "$container" sh -eu -c '
  nixpkgs=$(nix --extra-experimental-features "nix-command flakes" eval --impure --raw \
    --expr '\''(builtins.getFlake "path:/release/nix").inputs.nixpkgs.outPath'\'')
  nix --extra-experimental-features "nix-command flakes" profile install \
    "path:$nixpkgs#python3" "path:$nixpkgs#systemd"
  python3 /repo/zkapi-clientd/scripts/test-nix.py /release --service-output /service-units
  mkdir -p /opt /home/nix-test /run/user/1000 /run/systemd/system
  ln -s "$(readlink -f /root/.nix-profile)" /opt/tools
  echo "nix-test:x:1000:1000:Nix service test:/home/nix-test:/bin/sh" >> /etc/passwd
  echo "nix-test:x:1000:" >> /etc/group
  chown -R 1000:1000 /home/nix-test /run/user/1000
  chmod 700 /run/user/1000
  chown 1000:1000 /sys/fs/cgroup /sys/fs/cgroup/cgroup.procs /sys/fs/cgroup/cgroup.subtree_control
'
docker exec --user 1000:1000 \
  -e HOME=/home/nix-test -e XDG_RUNTIME_DIR=/run/user/1000 \
  -e PATH=/opt/tools/bin:/nix/var/nix/profiles/default/bin \
  "$container" python3 /repo/zkapi-clientd/packaging/nix/service-smoke.py /service-units
