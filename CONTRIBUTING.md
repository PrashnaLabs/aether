# Contributing to Aether

Thanks for helping. Bug reports, fixes, docs, and features are all welcome.

## Before you start

- **Bugs**: search [existing issues](https://github.com/PrashnaLabs/aether/issues)
  first, then open one with steps to reproduce.
- **Features or large changes**: open an issue to discuss the idea before
  writing code, so we can agree on the approach.
- **Security problems**: do not open a public issue. Follow [SECURITY.md](SECURITY.md).

Everyone taking part is expected to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Set up

Follow [Development](README.md#development) in the README, or the
[development guide](https://aether.prashnalabs.com/docs/contributing/development/)
on the site. In short:

```sh
cargo run --locked -p aether
```

## Before you open a pull request

```sh
make fmt        # cargo fmt --all
make lint       # cargo clippy --locked --workspace --all-targets
make test       # cargo test --locked --workspace
```

If you changed `scripts/` or `packaging/`, also run `make test-packaging`.
CI runs formatting, tests, and packaging on macOS and Linux.

## Conventions

- Unit tests for `crates/<crate>/src/<path>.rs` live in
  `crates/<crate>/tests/unit/<path>.rs`, never inline. See **Unit test file**
  in [GLOSSARY.md](GLOSSARY.md).
- `unsafe` code is denied workspace-wide.
- Use the terms in [GLOSSARY.md](GLOSSARY.md) in code and docs.
- UI changes should pass the checks in [docs/ux-rubric.md](docs/ux-rubric.md)
  in both light and dark themes. Include screenshots in the pull request.
- Larger designs go in `docs/superpowers/specs/`; lasting decisions go in
  `docs/adr/`.
- Commit messages use a type prefix: `feat:`, `fix:`, `docs:`, `refactor:`,
  `test:`, `ci:`, `chore:`.
- Keep pull requests focused on one change.

## License

Aether is released under the [MIT License](LICENSE). By contributing, you
agree that your contributions are licensed under the same terms.
