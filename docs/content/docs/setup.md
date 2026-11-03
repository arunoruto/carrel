---
title: "Full setup"
weight: 3
---

[Installing carrel](/docs/installation.md) gets you a working library. This page adds everything around it, in order, on Linux or macOS without Nix: text and figures from PDFs, semantic search, papers picked up as you download them, recommendations from Google Scholar, and sharing. Every step after the first is optional, and `carrel config` at the end shows what carrel found.

**The short way**: once carrel is installed (step 1) and the services you want to use are running, `carrel setup` asks for the settings below one at a time: the library folder, your group's server, your name and token there, syncing, Docling, formulas, Ollama, the web UI and papers from Downloads. Each question shows what is set now, or a default such as `you@your-machine` for your name, and Enter keeps it; `-` leaves a setting out. Comments and everything else in the config stay as they are, and the old config is kept in `config.toml.bak`. The TUI asks the same questions on its first start on a machine, and again with `,`.

## 1. carrel

With [Homebrew](https://brew.sh), on macOS (Apple silicon) or Linux:

```sh
brew install arunoruto/tap/carrel
```

It brings [uv](https://docs.astral.sh/uv/) along, which runs the text extractor. Without Homebrew, use the install script and install uv yourself:

```sh
curl -fsSL https://raw.githubusercontent.com/arunoruto/carrel/main/install.sh | sh
curl -LsSf https://astral.sh/uv/install.sh | sh
```

Or paste this to your AI agent, which then goes through this page with you:

```text
Install carrel and set it up for me, following
https://raw.githubusercontent.com/arunoruto/carrel/main/install-ai.md
```

## 2. Text, figures and formulas

carrel turns each PDF into Markdown and figures, which search, the readers and your AI use. Pick by your machine:

| Your machine | Extractor | Set up with |
|---|---|---|
| Any | PyMuPDF: fast, good for most papers | nothing: uv runs it |
| A Mac with Apple silicon | Docling: better tables, reading order and figures, on the Mac's GPU | `uv tool install docling` |
| An NVIDIA GPU, here or on a machine nearby | Docling as a server, docling-serve | Docker, below |

Docling on a plain CPU works too (`uv tool install docling`), at tens of seconds to minutes per paper. carrel uses Docling when it finds it and falls back to PyMuPDF where Docling fails, or for books over 150 pages.

**docling-serve on an NVIDIA GPU** needs the [NVIDIA Container Toolkit](https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/latest/install-guide.html) and a driver of version 580 or newer (CUDA 13; `nvidia-smi` shows it):

```sh
docker run -d --name docling --gpus all -p 5001:5001 --restart unless-stopped \
  -e DOCLING_SERVE_ARTIFACTS_PATH= -e DOCLING_SERVE_MAX_SYNC_WAIT=1800 \
  -v docling-models:/opt/app-root/src/.cache/huggingface \
  ghcr.io/docling-project/docling-serve:latest
```

The empty `DOCLING_SERVE_ARTIFACTS_PATH` lets it download models it doesn't ship with, such as the one for formulas, into the volume; `MAX_SYNC_WAIT` gives long papers more than two minutes. With an older driver, use `ghcr.io/docling-project/docling-serve-cu128` with a version tag (`v1.38.2`, ...): the CUDA images have no `latest`. Then tell carrel where it is, in `~/.config/carrel/config.toml` (on macOS too):

```toml
docling = "http://gpu-box:5001"
formulas = true   # formulas as LaTeX; worth it with a GPU, minutes per paper without
```

More in [the extractor](/docs/reference/configuration.md#the-extractor).

## 3. Semantic search

[Semantic search](/docs/guide/semantic-search.md) finds papers by meaning, and also ranks [recommendations](/docs/guide/recommendations.md). It needs [Ollama](https://ollama.com) with an embedding model, on your machine or one nearby:

```sh
ollama pull bge-m3
```

```toml
[embeddings]
# ollama = "http://gpu-box:11434"   # when it isn't on this machine
```

Then `carrel embed --all` once; new papers are embedded as they come.

## 4. In the background

```sh
carrel service install web       # the web UI at http://127.0.0.1:7700; `carrel web --get-url` prints the link
carrel service install consume   # adds the PDFs you download, and reads Scholar alerts (step 5)
```

These are systemd user units on Linux and launchd agents on macOS, started at login ([details](/docs/installation.md#running-in-the-background)). They take `PATH` from the shell you run this in, so run it where `uv`, `docling` and `ollama` are found. The consume service watches the folders configured at the time: after changing `[consume] folders`, run `carrel service install consume` again.

## 5. Recommendations from Google Scholar

Scholar's alert emails become [recommendations](/docs/guide/recommendations.md), ranked by how close they are to your library. Have your mail client save them to a folder (the guide shows Thunderbird), and add it to the watched folders:

```toml
[consume]
folders = ["~/Downloads", "~/Documents/scholar-alerts"]
```

## 6. Your AI and other machines

```sh
carrel integrate claude   # or opencode, codex, gemini, cursor
```

With a group's [carrel server](/docs/guide/server.md), connect to it; this sets up syncing too:

```sh
carrel connect https://papers.example.org
```

Without one, sync through an S3 bucket or a shared folder ([Sharing a library](/docs/guide/sharing.md)), then let it sync by itself every 10 minutes:

```sh
carrel service install sync
```

[Sharing with your group](/docs/guide/sharing.md#sharing-with-your-group) covers keys per person and what happens when two people edit the same file.

## Check

```sh
carrel config
```

lists what carrel found: the library, the extractor (`docling at http://gpu-box:5001 with formulas, else pymupdf`), the embedding model, tokens and the sync remote, or what's missing.
