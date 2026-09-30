#!/usr/bin/env bash
set -euo pipefail
umask 022 # Release artifacts contain public binaries/assets, never wallet state.

# Run on the target OS/architecture. Rust and proving assets are included.
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_dir=$(cd -- "$script_dir/../.." && pwd)
version=${1:?Usage: build-native.sh VERSION OUTPUT_DIRECTORY PREPARED_ZKAPI_DIRECTORY}
output_dir=${2:?Output directory required}
zkapi_dir=${3:?Prepared ZKAPI source directory required}
archive_only=${4:-}
[[ -z "$archive_only" || "$archive_only" = --archive-only ]] || { echo "Unknown build option: $archive_only" >&2; exit 1; }
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo 'Release version must be MAJOR.MINOR.PATCH, without a prefix.' >&2
    exit 1
fi
mkdir -p "$output_dir"
output_dir=$(cd -- "$output_dir" && pwd)
zkapi_dir=$(cd -- "$zkapi_dir" && pwd)
source "$repo_dir/zkapi-clientd/packaging/zkapi-source.env"
python3 "$script_dir/verify-zkapi-source.py" "$zkapi_dir" "$OA_COMPANION_COMMIT" "$OA_PROTOCOL_COMMIT"
build_os=$(go env GOOS)
build_arch=$(go env GOARCH)
if [[ "$build_os/$build_arch" != "$(go env GOHOSTOS)/$(go env GOHOSTARCH)" ]]; then
    echo 'Build on the native target; Go cross-compile overrides cannot cross-compile the Rust companion.' >&2
    exit 1
fi
case "$build_os/$build_arch" in
    darwin/amd64|darwin/arm64|linux/amd64|linux/arm64) ;;
    *) echo "Unsupported release target: $build_os/$build_arch" >&2; exit 1 ;;
esac
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
cd "$repo_dir/zkapi-clientd"
CGO_ENABLED=0 go build -trimpath -buildvcs=false -ldflags="-s -w -X main.version=$version" -o "$stage/zkapi-clientd" ./cmd/zkapi-clientd
cd "$zkapi_dir"
cargo build --release --locked --bin zkapi
python3 "$script_dir/verify-zkapi-source.py" "$zkapi_dir" "$OA_COMPANION_COMMIT" "$OA_PROTOCOL_COMMIT"
install -m 755 target/release/zkapi "$stage/zkapi-walletd"
mkdir -p "$stage/share/zkapi-clientd/proof-setup"
cp -R protocol/setup/v2/. "$stage/share/zkapi-clientd/proof-setup/"
for proof_file in request.pk request.vk withdrawal.pk withdrawal.vk manifest.json; do
    test -s "$stage/share/zkapi-clientd/proof-setup/$proof_file"
done
python3 "$script_dir/collect-release-notices.py" "$zkapi_dir" "$stage"
find "$stage/share" -type d -exec chmod 755 {} +
find "$stage/share" -type f -exec chmod 644 {} +
cp "$repo_dir/zkapi-clientd/LICENSE" "$stage/LICENSE"
cp "$repo_dir/zkapi-clientd/docs/CLI_PACKAGING.md" "$stage/CLI_PACKAGING.md"
cp "$repo_dir/zkapi-clientd/packaging/systemd/zkapi-clientd.service" "$stage/zkapi-clientd.service"
printf '%s\n' "$version" > "$stage/VERSION"
python3 "$script_dir/archive-release.py" "$stage" "$output_dir/zkapi-clientd_${version}_${build_os}_${build_arch}.tar.gz"

if [[ "$build_os" = linux && "$archive_only" != --archive-only ]]; then
    cd "$repo_dir"
    export OA_PACKAGE_ARCH="$build_arch" OA_PACKAGE_VERSION="$version" OA_PACKAGE_STAGE="$stage"
    for format in deb rpm; do
        go run github.com/goreleaser/nfpm/v2/cmd/nfpm@v2.47.0 package \
            --config zkapi-clientd/packaging/nfpm.yaml --packager "$format" \
            --target "$output_dir/zkapi-clientd_${version}_linux_${build_arch}.${format}"
    done
fi
