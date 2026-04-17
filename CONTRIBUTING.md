# Contributing

`mailing-list-cli` is in the spec phase. The fastest way to help right now is to shape what we build before any code lands.

## Right now

1. **Read [the research](./research/README.md)** — five dossiers covering modern newsletter platforms, marketing tools, Resend's API surface, deliverability and compliance at scale, and email template formats for AI agents.
2. **Open a [Discussion](https://github.com/paperfoot/mailing-list-cli/discussions)** if you spot something missing, want a feature on the roadmap, or disagree with a direction call.
3. **Comment on the [pinned roadmap issue](https://github.com/paperfoot/mailing-list-cli/issues)** if you want a specific command added or reshaped.

## Once the binary ships

Three steps:

1. Fork, branch, code.
2. Run `cargo test` and `cargo clippy` until both are green.
3. Open a PR. Keep it focused — one feature or one fix per PR.

### Conventions

- We follow the [agent-cli-framework](https://github.com/199-biotechnologies/agent-cli-framework) patterns. Read the framework README before adding new commands.
- All output respects `--json` and TTY auto-detection — no command writes plain text in piped mode.
- Errors carry an actionable `suggestion` field. A suggestion that doesn't work is a P0 bug.
- Exit codes are `0`, `1`, `2`, `3`, or `4`. Never invent new ones.
- No interactive prompts, ever. Use flags.
- Update `agent-info` whenever you add or change a command, and add an integration test that asserts the command appears.

### Tests

- Unit tests live next to the code.
- Integration tests verify the public CLI surface end-to-end against a recorded Resend fixture.
- A spec test ensures every command listed in `agent-info` is routable.

## Code of conduct

Be kind. Disagree on the merits. We're all trying to build a tool that makes a hard job easy.

## License

By contributing you agree your work is licensed under the [MIT license](./LICENSE).
