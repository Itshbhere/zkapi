#!/usr/bin/env bash
# Run on the Docker host; e.g. invoke this script through ssh rockypika.
set -euo pipefail

if [[ $# != 1 ]]; then
  echo "Usage: $0 RELEASE_DIRECTORY" >&2
  exit 2
fi
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
release=$(cd -- "$1" && pwd)
version=$(sed -n 's/^  version "\([^"]*\)"/\1/p' "$here/zkapi-clientd.rb")
architecture=$(docker info --format '{{.Architecture}}')
case "$architecture" in
  x86_64|amd64) target=amd64 ;;
  aarch64|arm64) target=arm64 ;;
  *) echo "Unsupported Docker architecture: $architecture" >&2; exit 1 ;;
esac
archive="zkapi-clientd_${version}_linux_${target}.tar.gz"
test -f "$release/$archive"
image="zkapi-brew-test:local"
container="zkapi-brew-test-$$"
cleanup() { docker rm -f "$container" >/dev/null 2>&1 || true; }
trap cleanup EXIT

docker build --tag "$image" --file "$here/Dockerfile.test" "$here"
# A real user systemd manager needs a private writable cgroup namespace.
# Do not bind-mount the host cgroup tree, Docker socket, or private state.
docker run --detach --name "$container" --privileged --cgroupns=private \
  --tmpfs /run --tmpfs /run/lock --tmpfs /tmp "$image" >/dev/null
ready=false
for _ in $(seq 1 30); do
  if docker exec "$container" systemctl is-active --quiet multi-user.target >/dev/null 2>&1; then
    ready=true
    break
  fi
  sleep 1
done
if [[ "$ready" != true ]]; then
  docker logs "$container"
  echo 'Container systemd did not start' >&2
  exit 1
fi
docker exec "$container" mkdir -p /opt/artifacts
docker cp "$release/$archive" "$container:/opt/artifacts/$archive"
docker exec "$container" loginctl enable-linger linuxbrew
docker exec "$container" systemctl start user@1000.service
docker exec --user linuxbrew \
  --env XDG_RUNTIME_DIR=/run/user/1000 \
  --env DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus \
  "$container" bash -lc 'bash /opt/zkapi-package-test/test-container.sh'
