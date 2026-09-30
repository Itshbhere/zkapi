# zkapi-clientd

A local OpenAI-compatible API, paid from your private ETH balance. No OA account or OpenRouter API key needed.

## Quick start

You need Git, Go 1.25+, Rust 1.93+, Python 3, a C/C++ compiler, CMake, pkg-config, and OpenSSL 3 development libraries. On macOS, install Xcode command-line tools and `brew install go rust cmake pkg-config openssl@3`. The installer builds both client binaries; the first build takes several minutes.

```sh
git clone https://github.com/OpenAnonymity/zkapi.git
cd zkapi/zkapi-clientd
./scripts/install-source.sh
export PATH="$HOME/.local/bin:$PATH"
```

Configure your wallet:

```sh
zkapi-clientd config
```

The default is **Ethereum Mainnet**, with direct HTTPS and no network proxy. Enter how many dollars to deposit, or press Enter for **$20**. Send the displayed ETH amount to the address or scan its QR code. The client watches for payment; when enough arrives, press Enter to deposit. Activation usually takes about 15 minutes.

Then start the API:

```sh
zkapi-clientd serve
```

Leave that terminal running. In Open WebUI or another OpenAI-compatible client, set the base URL to **`http://127.0.0.1:8787/v1`** and leave the API key empty (use `local` if the app requires a value). Select a model and chat. The endpoint accepts local connections only.

To withdraw, run `zkapi-clientd config --menu` and choose `withdraw`. It asks for the destination and waits for extra ETH for fees only if needed.

[More options, including Sepolia and Docker clients](docs/CLI_ZKAPI.md) · [Updates and builds](docs/CLI_PACKAGING.md) · [Privacy](docs/PRIVACY.md)
