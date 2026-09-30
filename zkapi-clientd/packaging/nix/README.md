# Nix release packaging

This directory is copied into the generated release packaging archive. The
assembler renders `package.nix.in` as `package.nix` from all four actual archive
SHA-256 digests. Use the generated directory, not the unrendered source tree.
The flake lock pins Nixpkgs; archive URLs pin the exact daemon release tag.

After extracting `zkapi-clientd-packaging.tar.gz`, install without root privileges:

```sh
nix profile install ./nix#zkapi-clientd
zkapi-clientd config
zkapi-clientd serve
```

You can also run `nix run ./nix -- --version` or build with `nix build ./nix`.
Nix flakes require the `nix-command` and `flakes` experimental features.
The package supports x86_64/aarch64 on Linux and macOS. Linux ELF dependencies
are patched into the Nix store and the matching companion/proof data are bundled.
The companion is selected from the package's own `bin` directory. No user state
is created by a build, installation, or activation.

## NixOS user service

Add the generated directory as a flake input (retain it in your configuration
repository or publish it separately as an immutable release flake):

```nix
inputs.zkapi-clientd.url = "path:./zkapi-clientd-release/nix";
```

Then include the module and explicitly choose the login users:

```nix
imports = [ inputs.zkapi-clientd.nixosModules.default ];
services.zkapi-clientd = {
  enable = true;
  users = [ "alice" ];
  # Optional; defaults to manual start.
  startAtLogin = true;
};
```

Log in as that user and run `ZKAPI_CLIENTD_CONFIG_DIR="$HOME/.config/zkapi-clientd" zkapi-clientd config`, then
`systemctl --user start zkapi-clientd`. The default private directory is
`~/.config/zkapi-clientd`. A custom runtime directory can be selected with
`services.zkapi-clientd.configDir`; initialize the same directory with
`ZKAPI_CLIENTD_CONFIG_DIR=/absolute/private/directory zkapi-clientd config`.

## Home Manager user service (Linux)

```nix
imports = [ inputs.zkapi-clientd.homeManagerModules.default ];
services.zkapi-clientd = {
  enable = true;
  startAtLogin = true;
};
```

Run `zkapi-clientd config` as the login user, then `systemctl --user start zkapi-clientd`.
The module respects Home Manager's `xdg.configHome`; initialize that same
directory with `ZKAPI_CLIENTD_CONFIG_DIR` if it differs from your shell's default.
The service stays stopped until the initialized `config.json` exists.
Use `journalctl --user -u zkapi-clientd` for metadata-only operational logs.
Neither module creates a root daemon, initializes/funds a wallet, or places
configuration, keys, or private recovery state in the Nix store.
Retain and back up the runtime directory separately; package removal and Nix
garbage collection do not remove it.

## Release validation

`zkapi-clientd/scripts/test-nix.py RELEASE_ARTIFACTS` evaluates all four packages and
builds/executes the current host's package from the local release archive. It
also evaluates the NixOS and Home Manager service declarations without starting
a daemon or creating user state. Run this check on each native release host.
Unlike generated manifests, the packaging templates cannot be installed directly.
