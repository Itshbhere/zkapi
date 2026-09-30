# Installation, updates and development

## One-command installation

Install or update the `0.1.0` prerelease:

```sh
curl -fsSL https://github.com/OpenAnonymity/zkapi/releases/download/clientd-v0.1.0/install.sh | bash
```

Then configure and serve:

```sh
zkapi-clientd config
zkapi-clientd serve
```

The command downloads a prebuilt native bundle and checks its SHA-256 before
activation. No compiler, Git or Python is needed. It assumes the installation's
`bin` directory is already on PATH. The default prefix is `~/.local`; use
`--prefix` for another writable absolute prefix. Configuration is separate from
installation unless `--setup` is supplied. A new profile defaults to Ethereum
Mainnet and direct HTTPS; `config --network sepolia` explicitly selects test ETH.

For updates, stop `serve`, rerun the same install command, then restart `serve`.
The previous bundle is retained, and configuration, signing keys and recovery
state remain in their private directory. The installer does not start services,
modify shell startup files or terminate running processes.

This exact-tag command works for prereleases. GitHub's repository-wide
`latest/download` URL excludes prereleases and may select unrelated operator
releases, so it is not used. The generated installer is pinned to its release.

Runtime requirements: macOS 13+ or Linux with glibc 2.39+, on amd64 or arm64;
Bash, curl, tar and SHA-256 tooling (`sha256sum` or `shasum`). Linux also needs
OpenSSL 3, libgcc and CA certificates.

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

Source builds are optional developer tooling; the end-user command above uses
prebuilt binaries. For local development, `./scripts/install-source.sh` builds
and installs this checkout. It requires Git, Go 1.25+, Rust 1.93+, Python 3,
C/C++ tools, CMake, pkg-config and OpenSSL 3 development headers/libraries.
On macOS install Xcode command-line tools and the corresponding Homebrew tools.

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
are not automatically published. Publish a client prerelease only after the
native, installer and package validation jobs succeed.

`install.sh --version MAJOR.MINOR.PATCH` selects a published native bundle from
`OpenAnonymity/zkapi`. Its release-generated form is pinned to its own version.
It supports install/update with the same prefix and optional `--setup`.
`--setup` runs configuration after activation; no configuration is performed
by default. Update the pinned README URL when publishing another client release.

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
