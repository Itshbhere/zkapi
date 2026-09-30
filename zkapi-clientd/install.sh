#!/usr/bin/env bash
# Release assembly pins this value before publishing install.sh with the assets.
# Keep execution in main: a truncated curl download must not start installation.
main() {
    set -euo pipefail
    umask 022
    export LC_ALL=C

    local release_version='@@VERSION@@'
    # Keep cleanup state outside main's locals: Bash 5 can unwind those before
    # the EXIT trap when an external command fails under errexit.
    prefix=${HOME:?HOME must be set}/.local
    temporary='' release_dir='' install_root='' activated=0 locked=0 created_links=''
    local platform architecture os_version libc_version
    local archive_name base_url expected actual entry mode
    local executable target current_target=''
    local setup=0 setup_network='' network_selected=0
    local local_archive='' local_sha256=''

    fail() { printf 'zkapi-clientd installer: %s\n' "$*" >&2; exit 1; }
    cleanup() {
        local executable target
        # A signal can arrive after the atomic rename but before activated=1.
        # Never delete the bundle already selected by current in that window.
        if [[ -n "$release_dir" && -L "$install_root/current" ]] &&
            [[ "$(readlink "$install_root/current")" = "releases/${release_dir##*/}" ]]; then
            activated=1
        fi
        if [[ "$activated" = 0 ]]; then
            for executable in $created_links; do
                target="$install_root/current/bin/$executable"
                if [[ -L "$prefix/bin/$executable" && "$(readlink "$prefix/bin/$executable")" = "$target" ]]; then
                    rm -f "$prefix/bin/$executable"
                fi
            done
            [[ -z "$release_dir" ]] || rm -rf "$release_dir"
        fi
        [[ -z "$temporary" ]] || rm -rf "$temporary"
        if [[ "$locked" = 1 ]]; then
            rm -f "$install_root/.install-lock/current"
            rmdir "$install_root/.install-lock"
        fi
    }

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --version)
                [[ $# -ge 2 ]] || fail '--version requires MAJOR.MINOR.PATCH.'
                release_version=$2; shift 2 ;;
            --prefix)
                [[ $# -ge 2 ]] || fail '--prefix requires an absolute directory.'
                prefix=$2; shift 2 ;;
            --archive)
                [[ $# -ge 2 ]] || fail '--archive requires a local native bundle.'
                local_archive=$2; shift 2 ;;
            --sha256)
                [[ $# -ge 2 ]] || fail '--sha256 requires the local archive checksum.'
                local_sha256=$2; shift 2 ;;
            --setup)
                setup=1; shift ;;
            --network)
                [[ $# -ge 2 ]] || fail '--network requires mainnet or sepolia.'
                setup_network=$2; network_selected=1; shift 2 ;;
            --help|-h)
                cat <<'HELP'
Install or update the zkAPI client CLI and its zkAPI companion for the current user.

Usage: bash install.sh [--version MAJOR.MINOR.PATCH] [--prefix ABSOLUTE_DIR]
                       [--setup [--network mainnet|sepolia]]
                       [--archive LOCAL_BUNDLE --sha256 SHA256]

The published script defaults to its own release version. The source script
requires --version. The default prefix is $HOME/.local; commands go in its bin/.
Rerun with the same prefix to update; private configuration and wallets are kept.
Add --setup to run guided configuration and funding after installation.
--network requires --setup; omitted networks are selected during guided setup.
Interactive answers come from your terminal, never the piped installer script.
Requires macOS 13+ or Linux with glibc 2.39+, curl, tar, and SHA-256 tooling.
Linux also needs OpenSSL 3, libgcc, and CA certificates.
--archive installs an explicitly selected source-built bundle without downloading;
--sha256 is required. Local builds are checked by running both bundled binaries.
On NixOS, use the Nix flake package instead of the native archive installer.
Does not run sudo or edit shell profiles. Default installation does not initialize
wallets or start services; --setup runs configuration and exits when it is ready.
HELP
                return ;;
            *) fail "Unknown argument: $1 (see --help)." ;;
        esac
    done

    [[ "$release_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] ||
        fail 'Use a published release installer, or supply --version MAJOR.MINOR.PATCH.'
    [[ "$prefix" = /* && "$prefix" != / && "$prefix" != *:* && "$prefix" != *$'\n'* && "$prefix" != *$'\r'* ]] ||
        fail '--prefix must be an absolute directory without colons or newlines, other than /.'
    if [[ "$network_selected" = 1 ]]; then
        [[ "$setup" = 1 ]] || fail '--network requires --setup.'
        [[ "$setup_network" = mainnet || "$setup_network" = sepolia ]] || fail '--network must be mainnet or sepolia.'
    fi
    [[ "$(id -u)" != 0 ]] || fail 'Run this installer as your normal user, without sudo.'
    if [[ -n "$local_archive" ]]; then
        [[ -f "$local_archive" && "$local_sha256" =~ ^[a-f0-9]{64}$ ]] ||
            fail '--archive requires an existing file and --sha256 with its lowercase SHA-256 checksum.'
        local_archive=$(cd -- "$(dirname -- "$local_archive")" && pwd -P)/$(basename -- "$local_archive")
    else
        [[ -z "$local_sha256" ]] || fail '--sha256 requires --archive.'
        command -v curl >/dev/null 2>&1 || fail 'Required command is missing: curl.'
    fi

    for executable in tar awk mktemp readlink chmod mkdir mv ln rm rmdir cat uname cp; do
        command -v "$executable" >/dev/null 2>&1 || fail "Required command is missing: $executable."
    done
    if command -v sha256sum >/dev/null 2>&1; then
        checksum() { sha256sum "$1"; }
    elif command -v shasum >/dev/null 2>&1; then
        checksum() { shasum -a 256 "$1"; }
    else
        fail 'Install sha256sum or shasum before running this installer.'
    fi

    case "$(uname -s)" in
        Darwin)
            platform=darwin
            os_version=$(sw_vers -productVersion) || fail 'Cannot determine the macOS version.'
            [[ "$os_version" =~ ^([0-9]+)\. ]] || fail 'Cannot determine the macOS version.'
            [[ -n "$local_archive" ]] || (( BASH_REMATCH[1] >= 13 )) || fail 'The native release requires macOS 13 or newer.' ;;
        Linux)
            platform=linux
            # NixOS uses store-specific ELF interpreters and library paths.
            # A new glibc alone does not make the native archive runnable there.
            [[ ! -e /etc/NIXOS ]] ||
                fail 'On NixOS, install the Nix flake package instead of this native archive (see zkapi-clientd/docs/CLI_PACKAGING.md in the repository).'
            libc_version=$(getconf GNU_LIBC_VERSION 2>/dev/null) ||
                fail 'The native Linux release requires glibc 2.39 or newer (musl/Alpine is unsupported).'
            [[ "$libc_version" =~ ^glibc[[:space:]]([0-9]+)\.([0-9]+)$ ]] || fail 'Cannot determine the glibc version.'
            [[ -n "$local_archive" ]] || (( BASH_REMATCH[1] > 2 || (BASH_REMATCH[1] == 2 && BASH_REMATCH[2] >= 39) )) ||
                fail 'The native Linux release requires glibc 2.39 or newer; build from source on older distributions.' ;;
        *) fail 'Supported platforms are macOS and Linux (AMD64 or ARM64).' ;;
    esac
    case "$(uname -m)" in
        x86_64|amd64) architecture=amd64 ;;
        arm64|aarch64) architecture=arm64 ;;
        *) fail 'Supported architectures are AMD64 and ARM64.' ;;
    esac
    # An Intel shell under Rosetta should still receive the Apple Silicon build.
    if [[ "$platform" = darwin && "$architecture" = amd64 ]] &&
        [[ "$(sysctl -n sysctl.proc_translated 2>/dev/null || true)" = 1 ]]; then
        architecture=arm64
    fi

    mkdir -p "$prefix"
    prefix=$(cd "$prefix" && pwd -P)
    install_root="$prefix/lib/zkapi-clientd"
    mkdir -p "$install_root/releases" "$prefix/bin"
    mkdir "$install_root/.install-lock" 2>/dev/null ||
        fail "Another install may be running. If it has stopped, remove $install_root/.install-lock and retry."
    locked=1
    trap cleanup EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM
    trap 'exit 129' HUP

    # Only replace launchers belonging to this installation, never package-manager files.
    for executable in zkapi-clientd zkapi-walletd; do
        target="$install_root/current/bin/$executable"
        if [[ -e "$prefix/bin/$executable" || -L "$prefix/bin/$executable" ]]; then
            [[ -L "$prefix/bin/$executable" && "$(readlink "$prefix/bin/$executable")" = "$target" ]] ||
                fail "$prefix/bin/$executable already exists and is not managed by this installer. Choose another --prefix."
        fi
    done
    if [[ -e "$install_root/current" || -L "$install_root/current" ]]; then
        [[ -L "$install_root/current" ]] || fail "$install_root/current is not an installer symlink."
        current_target=$(readlink "$install_root/current")
        [[ "$current_target" =~ ^releases/[0-9]+\.[0-9]+\.[0-9]+-(darwin|linux)-(amd64|arm64)\.[a-zA-Z0-9]+$ ]] ||
            fail "$install_root/current points outside the managed releases."
    fi

    temporary=$(mktemp -d "${TMPDIR:-/tmp}/zkapi-clientd-install.XXXXXX")
    archive_name="zkapi-clientd_${release_version}_${platform}_${architecture}.tar.gz"
    base_url="https://github.com/OpenAnonymity/zkapi/releases/download/clientd-v${release_version}"
    download() {
        curl --proto '=https' --proto-redir '=https' --tlsv1.2 --fail --silent --show-error \
            --location --retry 3 --connect-timeout 15 --max-time 1800 --output "$2" "$1"
    }
    if [[ -n "$local_archive" ]]; then
        printf 'Installing source-built zkAPI client %s for %s/%s…\n' "$release_version" "$platform" "$architecture"
        expected=$local_sha256
        cp "$local_archive" "$temporary/archive.tar.gz" || fail 'Could not copy the local bundle.'
    else
        printf 'Downloading zkAPI client %s for %s/%s…\n' "$release_version" "$platform" "$architecture"
        download "$base_url/SHA256SUMS" "$temporary/SHA256SUMS" ||
            fail "Could not download checksums for clientd-v$release_version. Check that this release is published."
        expected=$(awk -v name="$archive_name" '$2 == name { count++; hash=$1 } END { if (count != 1) exit 1; print hash }' "$temporary/SHA256SUMS") ||
            fail "The checksum manifest must contain exactly one entry for $archive_name."
        download "$base_url/$archive_name" "$temporary/archive.tar.gz" || fail 'The archive download failed; the installed version was not changed.'
    fi
    [[ "$expected" =~ ^[a-f0-9]{64}$ ]] || fail 'The release checksum is invalid.'
    actual=$(checksum "$temporary/archive.tar.gz") || fail 'Could not compute the archive checksum.'
    actual=${actual%% *}
    [[ "$actual" = "$expected" ]] || fail 'SHA-256 checksum mismatch; the installed version was not changed.'

    # Native bundles contain regular files and directories only. Reject unsafe paths
    # and links before extraction, even if a malformed archive has a valid checksum.
    tar -tzf "$temporary/archive.tar.gz" > "$temporary/files" || fail 'Cannot read the release archive.'
    local safe_path='^[A-Za-z0-9._/+@ -]+$'
    while IFS= read -r entry; do
        [[ "$entry" =~ $safe_path ]] || fail 'The release archive contains an invalid path.'
        case "$entry" in
            /*|..|../*|*/../*|*/..) fail 'The release archive contains an unsafe path.' ;;
            .|./*|*/./*|*/.|*//*) fail 'The release archive contains a noncanonical path.' ;;
        esac
    done < "$temporary/files"
    # Trailing slashes on directories are normal tar output. Apart from that,
    # each destination must be unique so later members cannot replace files
    # already inspected under the same name.
    awk '{ name=$0; sub(/\/$/, "", name); if (seen[name]++) exit 1 }' "$temporary/files" ||
        fail 'The release archive contains duplicate paths.'
    tar -tvzf "$temporary/archive.tar.gz" > "$temporary/types" || fail 'Cannot inspect the release archive.'
    while IFS= read -r mode; do
        case "$mode" in
            -*|d*) ;;
            *) fail 'The release archive contains a link or special file.' ;;
        esac
    done < "$temporary/types"
    mkdir "$temporary/unpacked"
    tar -xzf "$temporary/archive.tar.gz" --no-same-owner --no-same-permissions -C "$temporary/unpacked" || fail 'Cannot extract the release archive.'
    for entry in zkapi-clientd zkapi-walletd VERSION LICENSE CLI_PACKAGING.md zkapi-clientd.service \
        share/zkapi-clientd/build-info.json share/zkapi-clientd/third-party/dependencies.json \
        share/zkapi-clientd/proof-setup/request.pk share/zkapi-clientd/proof-setup/request.vk \
        share/zkapi-clientd/proof-setup/withdrawal.pk share/zkapi-clientd/proof-setup/withdrawal.vk \
        share/zkapi-clientd/proof-setup/manifest.json; do
        [[ -f "$temporary/unpacked/$entry" && -s "$temporary/unpacked/$entry" ]] || fail "Release archive is missing $entry."
    done
    [[ "$(cat "$temporary/unpacked/VERSION")" = "$release_version" ]] || fail 'The archive version does not match the requested release.'

    release_dir=$(mktemp -d "$install_root/releases/$release_version-$platform-$architecture.XXXXXX")
    chmod 755 "$release_dir"
    mkdir "$release_dir/bin"
    mv "$temporary/unpacked/zkapi-clientd" "$temporary/unpacked/zkapi-walletd" "$release_dir/bin/"
    mv "$temporary/unpacked/share" "$temporary/unpacked/LICENSE" "$temporary/unpacked/CLI_PACKAGING.md" "$temporary/unpacked/zkapi-clientd.service" "$temporary/unpacked/VERSION" "$release_dir/"
    chmod 755 "$release_dir/bin/zkapi-clientd" "$release_dir/bin/zkapi-walletd"
    [[ "$("$release_dir/bin/zkapi-clientd" version)" = "zkapi-clientd $release_version" ]] || fail 'The downloaded zkapi-clientd executable could not report the expected version.'
    "$release_dir/bin/zkapi-walletd" --help > "$temporary/companion-help" 2>&1 ||
        fail 'The zkAPI companion could not start. Check OS compatibility and installed OpenSSL 3/libgcc libraries; the installed version was not changed.'

    for executable in zkapi-clientd zkapi-walletd; do
        if [[ ! -L "$prefix/bin/$executable" ]]; then
            ln -s "$install_root/current/bin/$executable" "$prefix/bin/$executable"
            created_links="$created_links $executable"
        fi
    done
    ln -s "releases/${release_dir##*/}" "$install_root/.install-lock/current"
    # Rename the symlink itself, rather than following the previous directory link.
    if [[ "$platform" = darwin ]]; then
        mv -fh "$install_root/.install-lock/current" "$install_root/current"
    else
        mv -fT "$install_root/.install-lock/current" "$install_root/current"
    fi
    activated=1
    printf '\nInstalled zkapi-clientd %s to %s/bin\n' "$release_version" "$prefix"
    case ":${PATH:-}:" in
        *":$prefix/bin:"*) ;;
        *)
            # shellcheck disable=SC2016 # Print a command for the user's shell.
            printf 'Add this directory to PATH (and your shell profile to keep it):\n  export PATH=%q:"$PATH"\n' "$prefix/bin" ;;
    esac
    target=$(command -v zkapi-clientd || true)
    if [[ -n "$target" && "$target" != "$prefix/bin/zkapi-clientd" ]]; then
        printf 'Your PATH currently selects %s. Put %s/bin first to use this installation.\n' "$target" "$prefix"
    fi
    if [[ -n "$current_target" ]]; then
        printf 'Existing private configuration and wallet state were preserved.\n'
        printf 'Restart any running daemon to use the new version. Previous release retained at %s/%s.\n' "$install_root" "$current_target"
    fi
    [[ "$setup" = 1 ]] || printf 'Configure: zkapi-clientd config\nThen serve inference: zkapi-clientd serve\n'
    cleanup
    trap - EXIT
    if [[ "$setup" = 1 ]]; then
        local config_arguments=(config)
        [[ "$network_selected" = 0 ]] || config_arguments+=(--network "$setup_network")
        # Guided configuration discovers its matching companion through PATH. Keep
        # this installation ahead of any older pair without editing profiles.
        export PATH="$prefix/bin${PATH:+:$PATH}"
        printf '\nStarting guided setup…\n'
        exec "$prefix/bin/zkapi-clientd" "${config_arguments[@]}"
    fi
}

main "$@"
