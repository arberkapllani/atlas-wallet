<!--
Thank you for your contribution! Please make sure you have read CONTRIBUTING.md.
-->

## Summary

<!-- What does this PR do, in one or two sentences? -->

## Motivation

<!-- Why is this change needed? Link any related issue: Fixes #123 -->

## Changes

<!-- Bullet list of the user-visible / structural changes. -->

-

## Testing

<!-- How was this verified? Include relevant commands and outputs. -->

- [ ] `cargo test --workspace`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `pnpm --filter atlas-desktop check`
- [ ] `pnpm --filter atlas-desktop lint`

## Security review

- [ ] No private-key / seed material crosses the IPC boundary.
- [ ] No new `unwrap` / `expect` / `panic!` in non-test code.
- [ ] New dependencies have been considered for supply-chain risk.

## Checklist

- [ ] `CHANGELOG.md` updated under `## [Unreleased]`.
- [ ] Documentation updated (README / inline doc comments).
- [ ] Bindings regenerated (`cargo test -p atlas-desktop export_bindings`) if the IPC surface changed.
