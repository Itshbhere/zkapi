# zkapi-clientd

A local OpenAI-compatible API, paid from your private ETH balance. No OA account or OpenRouter API key needed.

## Quick start

Install or update:

```sh
curl -fsSL https://github.com/OpenAnonymity/zkapi/releases/download/clientd-v0.1.0/install.sh | bash
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

To update, stop `serve`, rerun the install command, then start `serve` again. Your wallet is preserved.

To withdraw, run `zkapi-clientd config --menu` and choose `withdraw`. It asks for the destination and waits for extra ETH for fees only if needed.

[More options, including Sepolia and Docker clients](docs/CLI_ZKAPI.md) · [Installation details](docs/CLI_PACKAGING.md) · [Privacy](docs/PRIVACY.md)
