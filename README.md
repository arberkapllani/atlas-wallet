# Exodus 2

Sovereign, multi-chain desktop wallet — a non-custodial, open-source cryptocurrency wallet for Windows, macOS, and Linux.

> **Status:** Phase 1 — scaffolding + Bitcoin + EVM (Ethereum, Polygon, BSC, Arbitrum, Optimism, Base).
> Solana, Cosmos, and other chains will be added in Phase 2. Swap (Phase 3) and hardware wallets (Phase 4) follow.

## Architecture

- **Backend:** Rust (Tauri 2 host process). All cryptographic material lives here. Private keys never reach the JavaScript layer.
- **Frontend:** SvelteKit + TailwindCSS rendered in the OS native WebView. Pure presentation/input layer.
- **IPC:** Type-safe bindings auto-generated with `tauri-specta`.
- **Persistence:** Encrypted vault file (`vault.bin`) using Argon2id → AES-256-GCM. Metadata in SQLite.

```
exodus-2/
├── Cargo.toml                # Rust workspace
├── apps/desktop/
│   ├── src/                  # SvelteKit UI
│   └── src-tauri/            # Tauri shell (Rust)
└── crates/
    ├── wallet-core/          # Mnemonic, BIP32/39/44/84, vault encryption
    ├── chain-traits/         # ChainProvider trait + shared types
    ├── chain-bitcoin/        # BTC (BIP84/P2WPKH, mempool.space)
    ├── chain-evm/            # Eth + L2s (alloy, EIP-1559)
    └── price-oracle/         # CoinGecko price feeds
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

- [x] Phase 1: BTC + EVM (Eth, Polygon, BSC, Arbitrum, Optimism, Base)
- [ ] Phase 2: Solana, Cosmos hub, Avalanche C-Chain
- [ ] Phase 3: Swap (1inch, Jupiter, THORChain) — KYC-free crypto-to-crypto
- [ ] Phase 4: Hardware wallets (Ledger, Trezor)
- [ ] Phase 5: Auto-update + signed builds + distribution

## License

Dual-licensed under either of:

- Apache License 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.
