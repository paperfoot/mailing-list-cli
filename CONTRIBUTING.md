# Contributing

`mailing-list-cli` is a shipped Rust binary, published to crates.io and the `199-biotechnologies/homebrew-tap` Homebrew tap. Contributions to commands, tests, and docs are welcome.

## How to contribute

1. Fork, branch, code.
2. Run `cargo test` and `cargo clippy` until both are green.
3. Open a PR. Keep it focused — one feature or one fix per PR.

For anything larger than a bug fix, open an [issue](https://github.com/paperfoot/mailing-list-cli/issues) first so the shape is agreed before you build. The [research dossiers](./research/README.md) explain why the tool is designed the way it is; the release process lives in [docs/release.md](./docs/release.md).

### Conventions

- We follow the [agent-cli-framework](https://github.com/paperfoot/agent-cli-framework) patterns. Read the framework README before adding new commands.
- All output respects `--json` and TTY auto-detection — no command writes plain text in piped mode.
- Errors carry an actionable `suggestion` field. A suggestion that doesn't work is a P0 bug.
- Exit codes are `0`, `1`, `2`, `3`, or `4`. Never invent new ones.
- No interactive prompts, ever. Use flags.
- Update `agent-info` whenever you add or change a command, and add an integration test that asserts the command appears.

### Tests

- Unit tests live next to the code.
- Integration tests verify the public CLI surface end-to-end against a stub email-cli script (`tests/fixtures/stub-email-cli.sh`) — this crate has no Resend code of its own; every API call goes through email-cli.
- A spec test ensures every command listed in `agent-info` is routable.

## Code of conduct

Be kind. Disagree on the merits. We're all trying to build a tool that makes a hard job easy.

## License

By contributing you agree your work is licensed under the [MIT license](./LICENSE).
