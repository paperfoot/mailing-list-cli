# Release Process

`mailing-list-cli` is a Rust binary. The supported package channels are:

- `cargo install mailing-list-cli --force`
- `brew update && brew upgrade mailing-list-cli`
- `cargo install --git https://github.com/paperfoot/mailing-list-cli --locked` for unreleased `main`

There are no `uv` or `bun` artifacts for this project.

## Automated Release

Pushing a `vX.Y.Z` tag runs `.github/workflows/release.yml`.

The workflow:

1. Checks out the tag.
2. Verifies `vX.Y.Z` matches `Cargo.toml` version `X.Y.Z`.
3. Runs `cargo fmt --check`, `cargo clippy`, `cargo test`, `cargo build --release`, and `cargo package --locked`.
4. Publishes the crate to crates.io if that version is not already published.
5. Updates `199-biotechnologies/homebrew-tap` with the tagged GitHub source tarball and SHA256.
6. Creates or updates the GitHub release.

Required secrets on `paperfoot/mailing-list-cli`:

- `CARGO_REGISTRY_TOKEN` (or `CRATES_IO_TOKEN`): crates.io token with publish rights for `mailing-list-cli`.
- `HOMEBREW_TAP_TOKEN`: GitHub token with `contents:write` access to `199-biotechnologies/homebrew-tap`.

## Manual Patch Release

Use this when a secret is missing or the automation needs repair:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features -- --test-threads=1
cargo publish --dry-run
git tag -a vX.Y.Z -m "vX.Y.Z"
git push origin main vX.Y.Z
cargo publish --locked
```

Then update the Homebrew formula with the SHA256 of:

```text
https://github.com/paperfoot/mailing-list-cli/archive/refs/tags/vX.Y.Z.tar.gz
```
