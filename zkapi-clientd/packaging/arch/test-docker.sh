#!/usr/bin/env bash
set -euo pipefail

# Run on a Linux Docker host. An optional directory of release archives avoids
# downloading them again; makepkg still checks the exact PKGBUILD checksums.
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
if (( $# > 1 )); then
    echo "Usage: $0 [RELEASE_CACHE_DIRECTORY]" >&2
    exit 2
fi
cache_args=()
if (( $# == 1 )); then
    cache_dir=$(cd -- "$1" && pwd)
    cache_args=(--mount "type=bind,src=$cache_dir,dst=/release-cache,readonly")
fi

container="zkapi-arch-$(date +%s)-$$"
created=false
cleanup() {
    result=$?
    trap - EXIT
    if "$created"; then
        if (( result != 0 )); then
            docker logs --tail 60 "$container" >&2 || true
            docker exec "$container" journalctl --no-pager -n 60 \
                _SYSTEMD_USER_UNIT=zkapi-clientd.service >&2 || true
        fi
        docker rm -f "$container" >/dev/null || true
    fi
    exit "$result"
}
trap cleanup EXIT

# A real systemd PID 1 and user manager exercise the installed service. Docker
# needs privileged mode for this setup; no host cgroup or wallet directories
# are mounted.
docker create --name "$container" --privileged --cgroupns=private \
    --tmpfs /run --tmpfs /run/lock \
    --mount "type=bind,src=$script_dir,dst=/package-source,readonly" \
    "${cache_args[@]}" "${ARCH_IMAGE:-archlinux:base-devel}" /sbin/init >/dev/null
created=true
docker start "$container" >/dev/null
docker exec "$container" bash /package-source/test-container.sh
