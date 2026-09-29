# Sepolia note-bound deployment (2026-09-22)

> Historical repair/validation record. The subsequent [native-only cleanup](../native-only-cleanup.md)
> removes legacy paths; the revision and deployment evidence below remains historical.


This isolated test deployment uses circuit `zkapi-v2-note-bound-v1`, protocol v2,
with a fresh demo billing token and immutable signing keys. The Groth16 setup is
for development and the freely mintable token has no value.

| Resource | Value |
| --- | --- |
| API / indexer | [Public endpoint](https://d24dltwirql2l5.cloudfront.net) |
| Public configuration | [config.json](https://d24dltwirql2l5.cloudfront.net/config.json) |
| Health | [Process health](https://d24dltwirql2l5.cloudfront.net/health) |
| AWS region | `us-west-1` |
| EC2 | `i-0c44b6d92cbf2df00` (`t4g.large`, Ubuntu 24.04 ARM64) |
| Elastic IP | `52.53.106.195` |
| CloudFront | `E5NVENI28E8DS` |
| Security group | `sg-04d333e43af93e842` |
| Vault | `0x94b4E26b292dB9A57A4B410E5b45B12eA14cbc1C` |
| Billing token | `0x046BA3224cb9C20e997456c4262f9376920283bC` |
| Groth16 adapter | `0xC19c12c1fac746f832Ce1690Af09e8b922E72101` |
| Poseidon library | `0x9C7c6A1E47bab710d8Fc1fC701251Aa25d1fdEb0` |
| Deployment block | `11761381` |
| Deployment transaction | `0x75b6beb8e01949d0ac5445ebeb799b0464c5bce83a282b973d28cc6543047156` |
| Owner / treasury | `0x801c5C530475155D4b00DaF77A889811e74f812A` |
| Challenge signer | `0x1Aa06876ECC91E4Bd4FE5238569Bff77e6825eB3` |
| Proxy request cap / native test lease reservation | `50000` micro-USD credits ($0.05) |
| Note lifetime / escape delay | 30 days / 24 hours |
| Protocol revision | `49164f68cbfbc65eda66a35bcaab2400952699e2` |
| Linux server SHA-256 | `b3a7e8935819f7bea2281889f36f39d3c45fa8333679b18683f5258a7315d2c0` |
| Linux challenger SHA-256 | `50c65f4bf25d20fef8ef4a2dcf59754043e7a348189f8a8589385063c2f28ca6` |

The server, indexer, challenge daemon, restricted signing sidecar, and gateway
run through `docker/aws/compose.yml`. Containers restart automatically. State
persists under `/var/lib/zkapi`; root-only configuration is under `/etc/zkapi`.
The VM has an encrypted 50 GiB gp3 root volume and requires IMDSv2. Only
CloudFront origin traffic reaches port 80; SSH is restricted to the deployment
operator's source address. Daemon and signing RPC ports are not published.

The OA key source is `https://54.200.38.68.sslip.io`, with verifier
`https://verifier2.openanonymity.ai`. Service credentials are separate from
public deployment metadata. The new stack uses its own database and seeds.

## Operations

On the VM:

```sh
cd /opt/zkapi
sudo docker compose --env-file /etc/zkapi/compose.env \
  -f docker/aws/compose.yml --profile challenge ps
sudo docker compose --env-file /etc/zkapi/compose.env \
  -f docker/aws/compose.yml --profile challenge logs --tail 100 challenger
```

The challenge signer needs Sepolia ETH. A single Merkle-tree transaction can
use roughly 7 million gas; budget for several concurrent escapes, including
fee fluctuations. The signer allows only zero-value challenge calls for this
vault and checks the chain, canonical ABI, sender, gas limit, and fee cap.
Its RPC is private to the container network.

## Verification

Vault identity, token decimals, request cap, signing keys and delays were read
back from Sepolia. The deployed Groth16 verifier bytecode matches the compiled
adapter exactly. Public proving-key hashes match local setup artifacts. Health,
indexer roots, route restrictions and signer rejection rules passed.

Live core lifecycle acceptance passed with the native client, public HTTPS
API/indexer, real Groth16 proofs and the deployed challenge daemon. Notes 0 and
1 each deposited 100,000 demo-token base units. One metered completion on note
0 returned HTTP 200 and charged 3 credits, leaving 99,997. Cooperative closes
returned 99,997 to note 0's destination and all 100,000 to note 1's destination;
the token receipts also record the 3-credit treasury payment. Both notes closed.

| Operation | Successful Sepolia transaction |
| --- | --- |
| Demo-token mint | [152b45a1…](https://sepolia.etherscan.io/tx/0x152b45a1406e549698caa8035d56730a57fa5eafcd591dad3783a45e64fce138) |
| Deposit approval | [6dbf7efb…](https://sepolia.etherscan.io/tx/0x6dbf7efbf59db776eb4c403c54e92774f7fa8b46ba2bd9de777f86f29617a3e7) |
| Note 0 deposit | [8cea4af8…](https://sepolia.etherscan.io/tx/0x8cea4af826819266a14c20bccd70df1509c65cc47bcc9af27fe86b38670efd9b) |
| Note 1 deposit | [6de0e12f…](https://sepolia.etherscan.io/tx/0x6de0e12f7cf80fa53b227241cf495fd4b393169fb1ee743b7a683191c384bc95) |
| Stale note 0 escape initiation | [ec12543b…](https://sepolia.etherscan.io/tx/0xec12543b301d202f35a187dca2e23630f5a955b09aeff4c7a2fb6766273e8995) |
| Automatic escape challenge | [96846265…](https://sepolia.etherscan.io/tx/0x96846265ee3c7ee1a6e7a3b29b0dff3b9b71f93264f40d232876b97a6ffa98da) |
| Note 1 cooperative close | [e648445f…](https://sepolia.etherscan.io/tx/0xe648445fbb7307343a59c061b4ab4d0e9cd731196b93bee9f31573e0f722ce08) |
| Note 0 cooperative close | [b913b9a2…](https://sepolia.etherscan.io/tx/0xb913b9a2c8ebb24180a910a8dcb14090e9d4550919260bf3c932673a2954a6ce) |

The challenge tested the historical-root repair: note 0's consumed genesis
state was saved before its request, and note 1 was deposited afterward. The
stale escape therefore began against a different root from the archived
request proof:

```text
Historical request root:
0x14a7b54647378d7a5021691311c1b5f88ea51e32f8c43378a92e3aad5004034c
Escape-start root:
0xf188f709e09f9f706a737df07d44f6d6d3fcd90a56a777d91496815a27c2c33
```

The live daemon submitted the archived proof with its original root and a
current restoration path. The confirmed challenge restored note 0 to Active
and cleared its pending withdrawal before the cooperative close. This run
tested challenge submission within the window; it did not wait 24 hours to
test an unchallenged escape finalization.

OA-org direct-mode acceptance also passed with native
`--require-oa-org-key-source`. The independently pinned verifier accepted the
station-issued key for a $0.05 reservation, a direct completion returned 2 tokens within its 8-token
limit, and explicit retirement produced both org and station signatures on the
final usage receipt. The key closed at `1790125464`, one second after retirement
was requested and 297 seconds before its provider expiry (`1790125761`). Usage
settled at 3 credits.

Note 2's [100,000-credit deposit](https://sepolia.etherscan.io/tx/0x3a8abac8853e4898ac830e9978d78ee284f0f3c7ff10aa25dc8efd52e3bb4d29)
then [closed cooperatively](https://sepolia.etherscan.io/tx/0xc076212c0405ba146c2dd6b1af853c3164e91d7b815ff1ed77f3771fa90c2a02),
returning 99,997 credits to the destination and 3 to the treasury. The Sepolia
SDK configuration now pins this accepted deployment and its migration guard
has been removed. Mainnet remains on its guarded historical configuration.

These results establish the native protocol and OA receipt flows. They do not
establish OA Chat UI/login behavior or a hosted browser application's model
budget configuration. The $0.05 value is the tested native lease reservation,
not an established universal OA-org limit. Larger browser-selected budgets
were not exercised; their availability depends on the external org policy.

Final checks found all three acceptance notes closed, the OA lease finalized,
no pending challenge submissions, and identical roots at the vault, indexer and
server. All five containers remained running.

Test gas included a [0.051 ETH proof-of-work faucet payout](https://sepolia.etherscan.io/tx/0xf5c920de94f1c2135d24c84cfffc566aae9b79b3ced5d756365b3e1e8906f04d).
Mining was stopped after the claim. The challenge signer retained
0.0127293222317019 Sepolia ETH after acceptance; refill it before sustained use.
