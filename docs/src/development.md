# Contributing

carrel is written in Rust. The code is a library (`src/lib.rs` and its modules) with three frontends: the CLI (`src/main.rs`), the TUI (`src/tui.rs`) and the MCP server (`src/mcp.rs`).

## Adding a backend

Where carrel can be extended, there is a trait to implement, and the rest of the code only uses the trait:

- `Remote` (`src/remote.rs`): where `carrel sync` copies files; S3 and folders so far.
- `ReferenceSource` (`src/refs.rs`): where references come from; Crossref, Semantic Scholar, OpenAlex and ADS so far.
- `Notifier` (`src/notify.rs`): how carrel tells you things; email so far. A new one (Telegram, say) is a struct with `name` and `send`, a field in `notify::Config` for its settings, and a line in `notifiers()`.

## Setup

With Nix, `nix develop` gives you Rust, clippy, rustfmt, rust-analyzer, mdBook and uv. Otherwise install a recent Rust (1.89 or newer) and uv.

```sh
cargo run -- --help
cargo fmt && cargo clippy --all-targets -- -D warnings
```

## Tests

```sh
cargo test                  # offline: unit tests, the library, the CLI binary and the MCP server
cargo test -- --ignored     # the tests that need internet (and uv), in tests/network.rs
```

The internet tests are marked `#[ignore = "needs internet"]`, so a plain `cargo test`, which is also what the Nix build runs in its sandbox, skips them. The offline tests use a fake extractor and keep their library, index and config in a temporary folder, so they need neither Python nor a home directory. Semantic Scholar and arXiv rate-limit; if internet tests fail with a 429, set `S2_API_KEY` or wait a few minutes.

## Docs

This site is an [mdBook](https://rust-lang.github.io/mdBook/) in `docs/`:

```sh
mdbook serve docs --open
```

## Releases

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org): `fix:` makes a patch release, `feat:` a minor one, a `!` (or `BREAKING CHANGE:`) a major one (before 1.0: a minor one). On every push to `main`, [release-please](https://github.com/googleapis/release-please) keeps a release pull request with the next version and changelog up to date; merging it tags the release, and the release workflow attaches binaries for Linux (x86_64, arm64) and macOS (Apple silicon).
