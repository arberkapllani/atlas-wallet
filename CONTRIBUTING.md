# Contributing to Atlas

Thanks for your interest in contributing! Atlas is a non-custodial,
multi-chain desktop wallet with a strict separation between the Rust
backend (where keys live) and the SvelteKit UI (presentation only).

## Development setup

See [README.md](README.md) for prerequisites and toolchain versions.

```bash
pnpm install
pnpm tauri dev
```

## Required checks before opening a PR

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd apps/desktop
pnpm check
pnpm lint
pnpm build:vite
```

CI runs all of the above plus `cargo-audit` and `cargo-deny` on every PR.

## Coding guidelines

### Rust
- No `unwrap()` / `expect()` / `panic!()` outside of `#[cfg(test)]`. Return
  `Result` and let the IPC layer convert errors with `error::AppError`.
- Crypto material must live behind `Zeroizing<...>` and never be `Clone`d
  unless absolutely required.
- Public IPC commands (`#[tauri::command]`) must accept simple, JSON-friendly
  argument types and use `specta::Type` so the TypeScript bindings stay in
  sync. Run `cargo test -p atlas-desktop export_bindings` after changing
  the IPC surface.
- Add unit tests in the same file as the code under `#[cfg(test)] mod tests`.

### Frontend (SvelteKit)
- Never bring private-key material into the JS layer. All cryptographic
  operations go through `$lib/api.ts`, which wraps `invoke` calls.
- Run `pnpm format` before committing.
- Keep components small; reusable primitives go under `src/lib/ui/`.

## Commit messages

Use Conventional Commits where reasonable:

```
feat(chain-evm): add support for Linea
fix(wallet-core): zeroize seed on lock
docs(README): update phase status
```

## Pull request checklist

- [ ] Tests added or updated.
- [ ] `cargo test --workspace` passes locally.
- [ ] `pnpm check` and `pnpm lint` pass locally.
- [ ] No new `unwrap`/`expect` in non-test code.
- [ ] No private-key material crosses the IPC boundary.
- [ ] `CHANGELOG.md` updated under the `## [Unreleased]` section.

## Reporting security issues

See [SECURITY.md](SECURITY.md). Do **not** open public issues for vulnerabilities.

## License

By contributing, you agree your contribution is licensed under both the
[MIT License](LICENSE-MIT) and the [Apache License 2.0](LICENSE-APACHE), at
the user's option.
