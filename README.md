# carrel

[![CI](https://github.com/arunoruto/carrel/actions/workflows/ci.yml/badge.svg)](https://github.com/arunoruto/carrel/actions/workflows/ci.yml)
[![Docs](https://github.com/arunoruto/carrel/actions/workflows/docs.yml/badge.svg)](https://arunoruto.github.io/carrel)
[![Release](https://img.shields.io/github/v/release/arunoruto/carrel)](https://github.com/arunoruto/carrel/releases/latest)

A research paper library for you and your AI. *(A carrel is the private study desk in a university library.)*

- **Add papers by PDF, DOI, arXiv id or title**, or import a `.bib`; metadata is looked up and arXiv PDFs are downloaded.
- **Plain files you own**: one folder per paper, named by its citation key, with `<key>.bib`, `<key>.pdf`, notes and supplementary material.
- **Readable by AI**: every PDF becomes `fulltext.md` and `figures/`, served to assistants over **MCP**.
- **AI writes drafts, you decide**: proposed summaries, keywords and notes wait for your approval.
- **Projects** whose bibliographies follow the citations in your LaTeX or Typst files.
- **Full-text search**, **citation graphs**, a **TUI**, and a health check (`carrel doctor`).

**Documentation: <https://arunoruto.github.io/carrel>**

## Install

Binaries for Linux (x86_64, arm64) and macOS (Apple silicon) are on the [releases page](https://github.com/arunoruto/carrel/releases/latest). With Nix:

```sh
nix run github:arunoruto/carrel
```

with [bin](https://github.com/marcosnils/bin): `bin install github.com/arunoruto/carrel`, or with Cargo: `cargo install --git https://github.com/arunoruto/carrel`. Outside of Nix, text extraction needs [uv](https://docs.astral.sh/uv/).

## Quick start

```sh
carrel add 1706.03762 10.1038/nature14539 ~/Downloads/paper.pdf
carrel search attention author:vaswani
carrel                                             # the TUI
claude mcp add --scope user carrel -- carrel mcp   # let Claude Code use your library
carrel project sync thesis ~/thesis/*.tex && carrel bib -p thesis > ~/thesis/refs.bib
```

## Development

`nix develop` (or Rust 1.88+ and uv), then `cargo test`; see [Contributing](https://arunoruto.github.io/carrel/development.html). Releases are cut automatically from [Conventional Commits](https://www.conventionalcommits.org).

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.
