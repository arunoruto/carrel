<a id="readme-top"></a>

[![CI][ci-shield]][ci-url]
[![Docs][docs-shield]][docs-url]
[![Release][release-shield]][release-url]
[![Issues][issues-shield]][issues-url]
[![License][license-shield]][license-url]

<br />
<div align="center">
<h3 align="center">carrel</h3>

  <p align="center">
    A research paper library for you and your AI.
    <br />
    <em>A carrel is the private study desk in a university library.</em>
    <br />
    <a href="https://arunoruto.github.io/carrel"><strong>Explore the docs »</strong></a>
    <br />
    <br />
    <a href="https://github.com/arunoruto/carrel/issues/new?labels=bug">Report Bug</a>
    &middot;
    <a href="https://github.com/arunoruto/carrel/issues/new?labels=enhancement">Request Feature</a>
  </p>
</div>

<details>
  <summary>Table of Contents</summary>
  <ol>
    <li>
      <a href="#about-the-project">About The Project</a>
      <ul>
        <li><a href="#clients-and-servers">Clients and servers</a></li>
        <li><a href="#built-with">Built With</a></li>
      </ul>
    </li>
    <li>
      <a href="#getting-started">Getting Started</a>
      <ul>
        <li><a href="#prerequisites">Prerequisites</a></li>
        <li><a href="#installation">Installation</a></li>
      </ul>
    </li>
    <li><a href="#usage">Usage</a></li>
    <li><a href="#contributing">Contributing</a></li>
    <li><a href="#license">License</a></li>
  </ol>
</details>

## About The Project

- **Add papers by PDF, DOI, arXiv id or title**, or import a `.bib`; metadata is looked up and arXiv PDFs are downloaded.
- **Plain files you own**: one folder per paper, named by its citation key, with `<key>.bib`, `<key>.pdf`, notes and supplementary material.
- **Readable by AI**: every PDF becomes `fulltext.md` and `figures/`, served to assistants over **MCP**.
- **AI writes drafts, you decide**: proposed summaries, keywords and notes wait for your approval.
- **Projects** whose bibliographies follow the citations in your LaTeX or Typst files.
- **Full-text search** with field filters, and **semantic search** by meaning, with embeddings from [Ollama](https://ollama.com) (bge-m3 or a model you pick).
- **Picks papers up from Downloads** (`carrel consume`), asking first or on its own, and emails you what it did.
- **Citation graphs**, a **TUI**, and a health check (`carrel doctor`).
- **Shared by a group**: `carrel sync` through an S3 bucket (Garage, MinIO, ...) or a folder, and `carrel serve` on a server (Docker image or NixOS module) for MCP over HTTP and background work.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

### Clients and servers

There is one program. `carrel` on your laptop is a **client**, `carrel serve` is a **server**, and both keep a full library of plain files. They never talk to each other directly: they meet at the **remote**, a bucket or folder that `carrel sync` copies files to and from. The search index is a cache on each machine and is never synced.

```text
   laptop A           laptop B                  server (carrel serve)
   carrel · TUI       carrel · TUI              rounds every few minutes
        │                  │                          │
        │ carrel sync      │ carrel sync              │ sync each round
        └──────────┬───────┴──────────────────────────┘
                   │  S3 over HTTPS (or a shared folder)
             ┌─────┴─────┐
             │  remote   │  Garage, MinIO, AWS: files only
             └───────────┘

   your AI ── carrel mcp (stdio) ──▶ the laptop's library
   any AI  ── HTTPS /mcp + token ──▶ the server's library
```

| | Client | Server |
|---|---|---|
| add, edit, approve drafts | you | AIs propose drafts over MCP |
| extract text, look up references, embed | on demand | for every paper still missing them, once for everyone |
| pick up new PDFs | from your Downloads, and tells you | from a drop folder with `auto = true`, logged only |
| notify | about your own Downloads | about its own trouble, such as Ollama going away |

A server is optional; without one, sync alone shares the library. See [Clients and servers][architecture-url] for the details.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

### Built With

* [![Rust][Rust-badge]][Rust-url]
* [![Ratatui][Ratatui-badge]][Ratatui-url]
* [![SQLite][SQLite-badge]][SQLite-url]
* [![Python][Python-badge]][Python-url] (text extraction with PyMuPDF)
* [![Ollama][Ollama-badge]][Ollama-url]
* [![Nix][Nix-badge]][Nix-url]
* [![Docker][Docker-badge]][Docker-url]

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## Getting Started

### Prerequisites

Outside of Nix and the Docker image, text extraction needs [uv](https://docs.astral.sh/uv/). Semantic search needs an [Ollama](https://ollama.com) with an embedding model (`ollama pull bge-m3`); everything else works without it.

### Installation

Binaries for Linux (x86_64, arm64) and macOS (Apple silicon) are on the [releases page][release-url].

* Nix
  ```sh
  nix run github:arunoruto/carrel
  ```
  or add `github:arunoruto/carrel` as a flake input and use its package, or its NixOS module for a server.
* [bin](https://github.com/marcosnils/bin)
  ```sh
  bin install github.com/arunoruto/carrel
  ```
* Cargo
  ```sh
  cargo install --git https://github.com/arunoruto/carrel
  ```
* Docker, for a server: `ghcr.io/arunoruto/carrel:latest` (the latest release) or `:edge` (main); see [Running a server](https://arunoruto.github.io/carrel/server.html).

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## Usage

```sh
carrel add 1706.03762 10.1038/nature14539 ~/Downloads/paper.pdf
carrel search attention author:vaswani
carrel search -s "how do shadows hide between grains"   # by meaning
carrel consume                                     # new PDFs in Downloads
carrel                                             # the TUI
claude mcp add --scope user carrel -- carrel mcp   # let Claude Code use your library
carrel project sync thesis ~/thesis/*.tex && carrel bib -p thesis > ~/thesis/refs.bib
carrel sync                                        # share through the remote
```

_For more, see the [documentation][docs-url]._

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## Contributing

`nix develop` (or Rust 1.89+ and uv), then `cargo test`; see [Contributing](https://arunoruto.github.io/carrel/development.html). Releases are cut automatically from [Conventional Commits](https://www.conventionalcommits.org).

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

[ci-shield]: https://img.shields.io/github/actions/workflow/status/arunoruto/carrel/ci.yml?branch=main&style=for-the-badge&label=CI
[ci-url]: https://github.com/arunoruto/carrel/actions/workflows/ci.yml
[docs-shield]: https://img.shields.io/github/actions/workflow/status/arunoruto/carrel/docs.yml?branch=main&style=for-the-badge&label=docs
[docs-url]: https://arunoruto.github.io/carrel
[release-shield]: https://img.shields.io/github/v/release/arunoruto/carrel?style=for-the-badge
[release-url]: https://github.com/arunoruto/carrel/releases/latest
[issues-shield]: https://img.shields.io/github/issues/arunoruto/carrel.svg?style=for-the-badge
[issues-url]: https://github.com/arunoruto/carrel/issues
[license-shield]: https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue?style=for-the-badge
[license-url]: https://github.com/arunoruto/carrel/blob/main/LICENSE-MIT
[architecture-url]: https://arunoruto.github.io/carrel/architecture.html
[Rust-badge]: https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white
[Rust-url]: https://www.rust-lang.org
[Ratatui-badge]: https://img.shields.io/badge/Ratatui-1E1E2E?style=for-the-badge&logo=ratatui&logoColor=white
[Ratatui-url]: https://ratatui.rs
[SQLite-badge]: https://img.shields.io/badge/SQLite-003B57?style=for-the-badge&logo=sqlite&logoColor=white
[SQLite-url]: https://sqlite.org
[Python-badge]: https://img.shields.io/badge/Python-3776AB?style=for-the-badge&logo=python&logoColor=white
[Python-url]: https://www.python.org
[Ollama-badge]: https://img.shields.io/badge/Ollama-000000?style=for-the-badge&logo=ollama&logoColor=white
[Ollama-url]: https://ollama.com
[Nix-badge]: https://img.shields.io/badge/Nix-5277C3?style=for-the-badge&logo=nixos&logoColor=white
[Nix-url]: https://nixos.org
[Docker-badge]: https://img.shields.io/badge/Docker-2496ED?style=for-the-badge&logo=docker&logoColor=white
[Docker-url]: https://www.docker.com
