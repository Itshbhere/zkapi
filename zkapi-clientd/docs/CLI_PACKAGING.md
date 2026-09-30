# Installation, updates and development

## Install from source

Until a `zkapi-clientd` binary release is published, the source installer is the
working installation path:

```sh
git clone https://github.com/OpenAnonymity/zkapi.git
cd zkapi/zkapi-clientd
./scripts/install-source.sh
export PATH="$HOME/.local/bin:$PATH"
zkapi-clientd config
zkapi-clientd serve
```

Use Git, Go 1.25+, Rust 1.93+, Python 3, C/C++ tools, CMake, pkg-config and
OpenSSL 3 development headers/libraries. On macOS install Xcode command-line
tools and `brew install go rust cmake pkg-config openssl@3`. Build as your
ordinary user. The script builds the Go frontend and an exact pinned Rust
wallet helper, bundles their proof assets and dependency notices, and activates
the bundle through the checksum-validating installer. It neither funds a wallet
nor starts inference. It does not change shell startup files.

For updates, stop `serve`, update this checkout with `git pull --ff-only`, rerun
`./scripts/install-source.sh`, and restart `serve`. Configuration and wallet
state live outside the installation and are preserved.

## Existing OA Chat wallets

The client has been moved from `OpenAnonymity/oa-chat/daemon` and renamed from
`oa-chat` to `zkapi-clientd`. Ticket mode was removed. The live deployment pins,
state schemas, management authentication, helper bridge and wallet recovery
semantics remain the same.

A saved zkAPI profile can be reused with `--config-dir`. The default-directory
selection also recognizes an existing OA Chat zkAPI profile when the new
location is absent, so a funded default wallet does not appear empty after the
rename. It uses that directory in place; it does not copy, delete or re-key it.
`OA_CHAT_CONFIG_DIR` remains a fallback if `ZKAPI_CLIENTD_CONFIG_DIR` is unset.
Ticket profiles and malformed/orphaned legacy state need explicit attention,
not silent conversion. Stop the old daemon before switching clients. Never run
two clients against one wallet. Keep old binaries for ticket or legacy token
recovery; the new installer uses separate executable and install names.

## Native bundle

The managed layout is:

```text
PREFIX/bin/zkapi-clientd -> PREFIX/lib/zkapi-clientd/current/bin/zkapi-clientd
PREFIX/bin/zkapi-walletd -> PREFIX/lib/zkapi-clientd/current/bin/zkapi-walletd
PREFIX/lib/zkapi-clientd/current -> releases/RELEASE_DIRECTORY
PREFIX/lib/zkapi-clientd/releases/RELEASE_DIRECTORY/
  bin/zkapi-clientd
  bin/zkapi-walletd
  share/zkapi-clientd/proof-setup/
  share/zkapi-clientd/build-info.json
  share/zkapi-clientd/third-party/
```

The default prefix is `~/.local`. Keep the two binaries and `share` directory
together. The installer checks both binaries and switches `current` atomically;
previous bundles remain available. It does not terminate a running daemon.
The helper is private implementation machinery; use the frontend's `config`
and `serve` commands.

## Maintainer builds and releases

```sh
./scripts/prepare-zkapi.sh /tmp/clientd-wallet-source
./scripts/build-native.sh 0.1.0 /tmp/clientd-artifacts /tmp/clientd-wallet-source
```

Run on the native target. Supported release targets are macOS 13+ and Linux
with glibc 2.39+, on amd64 or arm64. Linux runtime dependencies are OpenSSL 3,
libgcc and CA certificates. Local source builds use the host platform's ABI.

The client release namespace is `clientd-vMAJOR.MINOR.PATCH`, separate from
operator/SDK releases. The client release workflow assembles checksum-pinned
installers and native archives, Homebrew/AUR/Nix metadata, and Linux packages,
then creates a draft release for publication. Homebrew taps and AUR packages
are not automatically published. No client release is implied by the source
migration. Do not advertise an installation URL for an unpublished tag.

`install.sh --version MAJOR.MINOR.PATCH` selects a published native bundle from
`OpenAnonymity/zkapi`. Its release-generated form is pinned to its own version.
It supports install/update with the same prefix and optional `--setup`.
`--setup` runs configuration after activation; no configuration is performed
by default. Future binary release commands belong in the short README once
their assets are actually available.

The root Rust workspace intentionally has a newer native-only operator layout.
The frontend still prepares historical companion commit
`20aa542ae98e767c0507133fd34b12a56f5ccd3d` with protocol commit
`8b2d4e3da921f956e1eb6b93afbf722a877c060c` in a separate build directory and applies
its reviewed bridge/transport patches. Source verification checks exact commits
and diffs. These patches must not be applied to the root workspace. Proving
assets remain deployment-pinned; never regenerate them for installation.

## Checks

```sh
go test -race ./...
go vet ./...
python3 scripts/test-install.py
python3 scripts/test-prepare-zkapi.py
python3 scripts/test-packages.py
```

Native packaging and service-manager checks have extra platform dependencies;
see the client workflows. Fixture tests never need live funds. Historical OA
Chat acceptance records remain in that repository's Git history and are not
claims of a new zkapi-clientd release or live transaction test.
