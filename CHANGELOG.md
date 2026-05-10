# Changelog

All notable changes to Atlas will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- `SECURITY.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `CHANGELOG.md`.
- GitHub issue and pull-request templates.
- GitHub Actions release workflow that builds installers for Windows, macOS, and Linux on tag push.
- `pnpm audit` step in CI.
- Onboarding backup-verify quiz that confirms the user has saved the mnemonic.
- Anti-phishing phrase displayed on the unlock screen when configured.
- Transaction history route under `/history`.
- NFT gallery route under `/nfts`.
- `format` script for Prettier (`pnpm --filter atlas-desktop format`).

### Changed
- Tightened the Tauri Content Security Policy: `connect-src` now lists
  the concrete hosts Atlas needs (CoinGecko, mempool.space, public RPCs)
  rather than `https://*`.
- Tightened the default Tauri capability — file-system access scoped to
  `$APPDATA` and explicit user-picked paths.
- Pinned `tauri-specta` to an exact version (was caret-versioned on a
  release candidate).
- README updated to reflect the actual shipped scope (Solana, Cosmos,
  Cardano, Tron, hardware wallets, swaps, WalletConnect, AA-ERC4337,
  silent payments, Tor).
- Frontend `lint` script now runs Prettier (was previously broken — invoked
  uninstalled `eslint`).

### Fixed
- `pnpm-lock.yaml` regenerated to match `package.json` (CI with
  `--frozen-lockfile` would have failed on `jsqr`).
- Removed the stray `apps/desktop/package-lock.json` that conflicted with
  the pnpm lockfile.

## [0.1.0] - unreleased

Initial scaffolding.

[Unreleased]: https://github.com/arberkapllani/atlas-wallet/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/arberkapllani/atlas-wallet/releases/tag/v0.1.0
