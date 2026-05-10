# Atlas

Sovereign, multi-chain desktop wallet — a non-custodial, open-source cryptocurrency wallet for Windows, macOS, and Linux.

> **Status:** pre-1.0, **unaudited**. Do **not** store significant funds yet. See [SECURITY.md](SECURITY.md) and [AUDIT.md](AUDIT.md).

## What's in the box

**Chains:** Bitcoin (BIP84/Taproot/Silent Payments), Ethereum + EVM L2s
(Polygon, BSC, Arbitrum, Optimism, Base), Solana, Cosmos hub, Cardano, Tron.

**Trading:** in-app swap aggregator across 1inch, Jupiter, THORChain and
ChangeNow with a unified routing layer; MoonPay on/off-ramp.

**Wallet types:** hot, watch-only, hardware (Ledger, Trezor), multisig BTC
(PSBT) and EVM (Safe), ERC-4337 smart-account support, Shamir backups.

**Privacy & safety:** Tor transport, anti-phishing phrase, address-poisoning
detection, calldata + EIP-712 risk decoding, approvals dashboard, blocklist,
spend limits, custom RPC + node policy enforcement.

**Other:** WalletConnect v2, dApp registry, NFT gallery, staking aggregator,
PnL with multiple accounting methods, transaction history, contacts &
address book, encrypted notes per tx, biometric unlock, recovery drills.

> Not every feature is exposed in the UI yet — the Rust backend ships ahead
> of the SvelteKit surface. See [CHANGELOG.md](CHANGELOG.md) for status.

## Architecture

- **Backend:** Rust (Tauri 2 host process). All cryptographic material lives here. Private keys never reach the JavaScript layer.
- **Frontend:** SvelteKit + TailwindCSS rendered in the OS native WebView. Pure presentation/input layer.
- **IPC:** Type-safe bindings auto-generated with `tauri-specta`.
- **Persistence:** Encrypted vault file (`vault.bin`) using Argon2id → AES-256-GCM. Metadata in SQLite.

```
atlas/
├── Cargo.toml                # Rust workspace
├── apps/desktop/
│   ├── src/                  # SvelteKit UI
│   └── src-tauri/            # Tauri shell (Rust)
└── crates/
    ├── wallet-core/          # Mnemonic, BIP32/39/44/84, vault encryption
    ├── chain-traits/         # ChainProvider trait + shared types
    ├── chain-bitcoin/        # BTC (BIP84/P2WPKH, mempool.space)
    ├── chain-evm/            # Eth + L2s (alloy, EIP-1559)
    ├── chain-solana/  chain-cosmos/  chain-cardano/  chain-tron/
    ├── exchange-router/  exchange-1inch/  exchange-jupiter/  exchange-thorchain/  exchange-changenow/
    ├── walletconnect/  dapp-registry/  aa-erc4337/  multisig-btc/  multisig-evm/
    ├── hardware-ledger/  hardware-trezor/
    ├── tor/  silent-payments/  phishing/  address-poisoning/  approvals/  calldata/  eip712/
    ├── price-oracle/  tx-history/  pnl/  staking/  nft-gallery/  contacts/  txnotes/
    └── ... (60 crates total)
```

## Prerequisites

| Tool | Version | Why |
|---|---|---|
| Rust | 1.78+ stable | Backend |
| Node.js | 18+ | Frontend tooling |
| pnpm | 9+ | Workspace package manager |
| Microsoft Visual Studio Build Tools | latest | Required by Rust on Windows |
| WebView2 | (preinstalled on Windows 11) | Tauri runtime |

### Install on Windows

```powershell
winget install Rustlang.Rustup
winget install OpenJS.NodeJS
npm install -g pnpm
# Install MSVC build tools (~5 GB):
winget install Microsoft.VisualStudio.2022.BuildTools --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

### Install on macOS

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
brew install node pnpm
xcode-select --install
```

### Install on Linux (Debian/Ubuntu)

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
sudo apt install -y libwebkit2gtk-4.1-dev libssl-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev
npm install -g pnpm
```

## Run

```bash
pnpm install            # frontend deps
pnpm tauri dev          # desktop dev with HMR
```

## Build a release binary

```bash
pnpm tauri build
# Windows: apps/desktop/src-tauri/target/release/bundle/msi/*.msi
# macOS:   apps/desktop/src-tauri/target/release/bundle/dmg/*.dmg
# Linux:   apps/desktop/src-tauri/target/release/bundle/{appimage,deb}
```

## Security model

1. **Non-custodial.** The mnemonic is generated locally with the OS CSPRNG and never leaves your machine.
2. **At rest:** seed is stored encrypted with AES-256-GCM. The key is derived from your password with Argon2id (m=64MiB, t=3, p=4).
3. **In memory:** decrypted seed material is held in `Zeroizing<...>` buffers and erased on lock/quit.
4. **No telemetry.** No analytics, no remote logging, no third-party trackers.
5. **RPC:** uses public nodes by default. Configurable per chain — point to your own node (e.g. `127.0.0.1:8545`) at any time.

> **Audit status:** unaudited. Do **not** store significant funds yet.

## Roadmap

- [x] Multi-chain backend (BTC, EVM L2s, Solana, Cosmos, Cardano, Tron) — code complete, UI surfacing in progress.
- [x] Swap aggregator + on-ramp — backend wired, basic UI shipped.
- [x] Hardware wallets (Ledger, Trezor) — backend wired; pairing UX in progress.
- [x] WalletConnect v2 + dApp registry — backend wired.
- [ ] Third-party security audit.
- [ ] Code-signed installers (Authenticode + Apple Developer ID + GPG).
- [ ] Signed auto-updater (`tauri-plugin-updater`).
- [ ] 1.0 release.

## License

Dual-licensed under either of:

- Apache License 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.

