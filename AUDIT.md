# Atlas Wallet — Pre-Release Audit Report

**Date:** 2026-05-10  
**Repository:** `arberkapllani/atlas-wallet` (branch `main`)  
**Scope:** Full pre-publication audit — build, tests, lints, code health, configuration, security posture, release readiness.  
**Verdict:** **Technically green-light.** All builds, tests, and type checks pass. Outstanding items before public distribution are operational (signing, third-party security audit, release pipeline) — not code defects.

---

## 1. Verification matrix

| Check | Command | Result |
|---|---|---|
| Rust compile | `cargo check --workspace --all-targets` | ✅ 0 errors / 0 warnings |
| Rust lint | `cargo clippy --workspace --all-targets` | ✅ 0 errors / 0 warnings |
| Rust tests | `cargo test --workspace --no-fail-fast` | ✅ **606 passed / 0 failed** across 111 suites |
| Frontend type check | `pnpm --filter atlas-desktop check` | ✅ 0 errors / 0 warnings |
| Frontend lint | `pnpm --filter atlas-desktop lint` (Prettier) | ✅ clean (after format) |
| Frontend build | `pnpm --filter atlas-desktop build:vite` | ✅ built in ~12s, adapter-static OK |

Toolchain at audit time: `rustc 1.95.0`, `pnpm 9.12.0`, Node 20+ expected, `tauri` 2.0.

---

## 2. Project shape

- **Workspace:** 60 Rust crates + 1 Tauri host (`apps/desktop/src-tauri`) + 1 SvelteKit frontend (`apps/desktop`).
- **Architecture (verified):** All cryptographic material is held in the Rust process. The frontend communicates only via type-safe `tauri-specta` IPC (`apps/desktop/src/lib/bindings.ts` is auto-generated on debug start and via `cargo test -p atlas-desktop export_bindings`).
- **IPC surface:** ~190 commands and 4 events registered in [apps/desktop/src-tauri/src/main.rs](apps/desktop/src-tauri/src/main.rs).
- **Persistence:** AES-256-GCM vault + Argon2id KDF for seed material; SQLite for metadata.

The repository is far more complete than the README suggests (see §5).

---

## 3. Issues found and fixed during this audit

### 3.1 `pnpm-lock.yaml` out of sync (HIGH — would break CI)
`apps/desktop/package.json` declared `jsqr` and other deps that were missing from the lockfile. CI using `--frozen-lockfile` would fail.  
**Fix:** Lockfile regenerated.

### 3.2 Stray duplicate lockfile (MEDIUM)
`apps/desktop/package-lock.json` (npm format) coexisted with `pnpm-lock.yaml`. This causes inconsistent installs and tooling confusion.  
**Fix:** File removed.

### 3.3 Broken `pnpm lint` script (MEDIUM)
The script invoked `prettier` and `eslint`, neither installed and neither configured. Running `pnpm lint` exited with `'prettier' is not recognized`.  
**Fix:**
- Simplified script to `prettier --check .` and added a `format` script for `prettier --write .`.
- Added `prettier` + `prettier-plugin-svelte` to `devDependencies`.
- Created [apps/desktop/.prettierrc.json](apps/desktop/.prettierrc.json) and [apps/desktop/.prettierignore](apps/desktop/.prettierignore).

### 3.4 Inconsistent formatting across 62 files (LOW, cosmetic)
Resolved by running `pnpm format`.

### 3.5 CI frontend job did not lint or build (LOW)
The existing [.github/workflows/ci.yml](.github/workflows/ci.yml) ran `pnpm check` only, leaving lint and build unverified.  
**Fix:** Added `pnpm lint` and `pnpm build:vite` to the frontend job.

---

## 4. Code-health observations

### 4.1 No `TODO`/`FIXME`/`unimplemented!()` in production paths
Searches across all 60 crates and the Tauri shell turned up no unfinished markers in production code. The only `panic!` calls are inside `#[cfg(test)]` test helpers — acceptable.

### 4.2 No `console.log` leakage in frontend
Only six `console.warn` / `console.error` calls remain, all guarding genuine error branches (e.g. RPC fee fetch failure). Acceptable for desktop diagnostics.

### 4.3 `@ts-ignore`/`@ts-expect-error` usage
A single `@ts-expect-error` in [apps/desktop/src/lib/ui/QrCode.svelte](apps/desktop/src/lib/ui/QrCode.svelte) for the `qrcode` package missing bundled types — well-commented, harmless.

### 4.4 Tests cover all crates with substance
Highest coverage by crate (sample): `address-poisoning` 17, `phishing` 16, `eip712` 16, `tor` 16, `silent-payments` 16, `pnl` 15, `staking` 15, `slippage` 14, `coincontrol` 13, `walletconnect` 13.

---

## 5. Documentation drift (HIGH visibility, LOW risk)

[README.md](README.md) advertises:

> Phase 1 — scaffolding + Bitcoin + EVM. Solana, Cosmos, and other chains will be added in Phase 2. Swap (Phase 3) and hardware wallets (Phase 4) follow.

But [Cargo.toml](Cargo.toml) already wires the production code for **all** of these: `chain-solana`, `chain-cosmos`, `chain-cardano`, `chain-tron`, `hardware-ledger`, `hardware-trezor`, `exchange-1inch`, `exchange-jupiter`, `exchange-thorchain`, `exchange-changenow`, `aa-erc4337`, `walletconnect`, `silent-payments`, `tor`, `nft-gallery`, `staking`, `multisig-btc`, `multisig-evm`, etc.

**Action:** Update README and the roadmap section before launch. Misleading scope statements undermine user trust and bias third-party reviewers.

---

## 6. Security posture review

> Atlas correctly states `Audit status: unaudited` in README. Below are findings from this *static* review only — they do not replace a professional audit.

### 6.1 Threat-model defaults — **good**
- Non-custodial: seed generated locally with OS CSPRNG.
- Argon2id (m=64MiB, t=3, p=4) → AES-256-GCM is appropriate.
- `Zeroizing<...>` buffers used in `wallet-core`.
- No telemetry / analytics dependencies discovered.

### 6.2 CSP is too permissive — **MEDIUM**
[apps/desktop/src-tauri/tauri.conf.json](apps/desktop/src-tauri/tauri.conf.json):
```json
"csp": "default-src 'self'; img-src 'self' data: https:; style-src 'self' 'unsafe-inline'; connect-src 'self' https://*"
```
`connect-src https://*` allows exfiltration to any HTTPS host if any frontend dependency is ever compromised.  
**Recommendation:** Restrict to a concrete allowlist of needed hosts (CoinGecko, mempool.space, the configured RPCs, MoonPay, WalletConnect relays, etc.). Investigate moving truly dynamic outbound traffic (user-configurable RPCs) to the Rust side via `reqwest` so the WebView never needs `connect-src` to arbitrary hosts.

### 6.3 Tauri capability scope — **LOW**
[apps/desktop/src-tauri/capabilities/default.json](apps/desktop/src-tauri/capabilities/default.json) grants `fs:default` and `dialog:allow-open|save` window-wide. Consider scoped FS permissions limited to `$APPDATA` and explicit user-picked paths.

### 6.4 Dependency hygiene — **LOW/MEDIUM**
- `tauri-specta` is on `2.0.0-rc.24` (release candidate). Pin it strictly until it ships 2.0.
- CI already runs `cargo-audit` and `cargo-deny` (good). Make sure `deny.toml` is reviewed and any allowed advisories are justified with a comment.
- A frontend `pnpm audit` step is **not** in CI — consider adding (advisory only).

### 6.5 RPC defaults
RPC endpoints default to public infra. Ensure no API key is bundled or auto-included in shipped binaries. (Verified: no API keys present in repo.)

---

## 7. Release readiness — **outstanding items**

These are **must-do before public distribution**:

1. **Third-party security audit.** Critical scope: `wallet-core` (vault, BIP32/39/84), `chain-bitcoin` (PSBT, silent payments), `chain-evm` (EIP-1559/EIP-712 signers), `walletconnect`, `aa-erc4337`, `multisig-*`, `shamir`. Until then, README must keep the "do not store significant funds" disclaimer prominent.

2. **Code signing & notarization** (none present in repo):
   - Windows: Authenticode certificate (EV strongly preferred for SmartScreen reputation).
   - macOS: Apple Developer ID + notarization + stapling.
   - Linux: GPG-sign `.deb` packages and AppImage `.zsync`.

3. **Auto-updater.** `tauri.conf.json` shows `"plugins": {}`. Add `tauri-plugin-updater` with a hard-coded **public** signing key checked into the repo so users get verified updates.

4. **Release pipeline.** No release workflow exists in `.github/workflows/`. Add one that:
   - Triggers on tags `v*`.
   - Builds `tauri build` on each of `windows-latest`, `macos-latest` (universal), `ubuntu-latest`.
   - Signs/notarizes with secrets.
   - Uploads `.msi`, `.dmg`, `.AppImage`, `.deb` to a GitHub Release.
   - Generates and signs the `latest.json` manifest for the updater plugin.

5. **README accuracy.** See §5 — bring it in line with what's shipped.

6. **License & SBOM.** Dual MIT/Apache-2.0 licenses are present. Consider generating an SBOM (e.g. `cargo cyclonedx`) per release.

7. **Reproducible builds (stretch).** Pin Rust toolchain (`rust-toolchain.toml` already present — verify MSRV and CI match), pin pnpm version, use `Cargo.lock` in CI build.

---

## 8. Files modified by this audit

- [apps/desktop/package.json](apps/desktop/package.json) — fixed `lint` script, added `format`, added `prettier` + `prettier-plugin-svelte`.
- [apps/desktop/.prettierrc.json](apps/desktop/.prettierrc.json) — new.
- [apps/desktop/.prettierignore](apps/desktop/.prettierignore) — new.
- `pnpm-lock.yaml` — regenerated to match `package.json`.
- `apps/desktop/package-lock.json` — removed (npm/pnpm collision).
- 62 `.svelte` / `.ts` / `.js` / `.json` files — Prettier-formatted.
- [.github/workflows/ci.yml](.github/workflows/ci.yml) — added `pnpm lint` and `pnpm build:vite` to the frontend job.

---

## 9. Reproducing this audit

```powershell
# Rust
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast

# Frontend
cd apps/desktop
pnpm install
pnpm check
pnpm lint
pnpm build:vite
```

All seven commands must exit with code 0. They do, as of this report.

---

## 10. Summary

**The codebase is in excellent shape for an unaudited pre-1.0 product.** No compile errors, no clippy warnings, no failing tests, no TODOs in production code, clean type-checked frontend, and a thoughtful architecture (Rust-only key custody, type-safe IPC). The fixes applied above remove the only mechanical blockers to a green CI run.

The remaining gap to a public 1.0 release is **not** code quality — it's the **operational release pillars**: external security audit, code signing, signed auto-updater, and an end-to-end release pipeline. Once those are in place, Atlas is ready to ship.
