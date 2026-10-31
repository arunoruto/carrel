---
title: "Installation"
weight: 1
aliases: ["/installation.html"]
---

carrel runs on Linux (x86_64 and arm64) and macOS (Apple silicon). On Windows, use WSL.

## Install script

On Linux and macOS:

```sh
curl -fsSL https://raw.githubusercontent.com/arunoruto/carrel/main/install.sh | sh
```

It downloads the latest release for your platform, checks its checksum and puts `carrel` in `~/.local/bin`. `CARREL_INSTALL_DIR=/somewhere` picks another folder and `CARREL_VERSION=v0.2.0` another release. Run it again to update. The script is short, so you can [read it](https://github.com/arunoruto/carrel/blob/main/install.sh) first.

**AI agents**: tell yours *"Install carrel and set it up for me, following https://raw.githubusercontent.com/arunoruto/carrel/main/install-ai.md"*. That guide walks it through installing carrel, the config, importing your papers and connecting itself.

## Prebuilt binaries

Download the archive for your platform from the [latest release](https://github.com/arunoruto/carrel/releases/latest), unpack it and put `carrel` on your `PATH`:

```sh
tar xzf carrel-v*-x86_64-unknown-linux-musl.tar.gz
install carrel-v*/carrel ~/.local/bin/
```

| Platform | Archive |
|---|---|
| Linux, x86_64 | `x86_64-unknown-linux-musl` (static, runs on any distribution) |
| Linux, arm64 | `aarch64-unknown-linux-musl` (static) |
| macOS, Apple silicon | `aarch64-apple-darwin` |

Each archive comes with a `.sha256` checksum.

## With bin

[bin](https://github.com/marcosnils/bin) picks the right archive for your platform, unpacks it and keeps track of the version:

```sh
bin install github.com/arunoruto/carrel
bin update carrel                          # later, to get a new release
```

## Nix

The repository is a flake:

```sh
nix run github:arunoruto/carrel           # try it
nix profile install github:arunoruto/carrel
```

or as a flake input, using its package or overlay:

```nix
{
  inputs.carrel.url = "github:arunoruto/carrel";
  # environment.systemPackages = [ inputs.carrel.packages.${pkgs.system}.default ];
  # or: nixpkgs.overlays = [ inputs.carrel.overlays.default ];  then pkgs.carrel
}
```

The Nix package comes with its text extractor (Python with pymupdf4llm from nixpkgs), so nothing is downloaded at runtime and extraction works offline. For [Docling](/docs/reference/configuration.md#the-extractor) instead, add `pkgs.docling` to your packages.

## macOS

Use the install script above, or Nix (with [nix-darwin](https://github.com/nix-darwin/nix-darwin) or Home Manager). There is no Homebrew formula. Background services are launchd agents, from `carrel service` or the Home Manager module (see below). The first time `carrel consume` reads Downloads, macOS asks for permission; if the prompt never comes, allow carrel under System Settings → Privacy & Security → Files and Folders.

## Docker

For a server, the image `ghcr.io/arunoruto/carrel` runs [`carrel serve`](/docs/guide/server.md) and comes with the extractor:

```sh
docker run --rm -it -v ~/Documents/carrel:/library ghcr.io/arunoruto/carrel:latest list
```

## With Cargo

```sh
cargo install --git https://github.com/arunoruto/carrel
```

This carrel has no [web UI](/docs/guide/web.md): `carrel web` serves only its API. The binaries, Nix and Docker have it. To build it in yourself, with [Node.js](https://nodejs.org):

```sh
git clone https://github.com/arunoruto/carrel && cd carrel
(cd web && npm ci && npm run build)
cargo install --path .
```

## The text extractor

Turning PDFs into text and figures is done by an external program (see [Configuration](/docs/reference/configuration.md#the-extractor)): [Docling](https://github.com/docling-project/docling) if it is installed, else [pymupdf4llm](https://pypi.org/project/pymupdf4llm/). Outside of Nix and Docker, pymupdf4llm runs through [uv](https://docs.astral.sh/uv/), so **install uv** (or Docling); uv fetches pymupdf4llm on first use. Without either, papers are still added, just without their text.

## Running in the background

`carrel web` (the [web UI](/docs/guide/web.md)) and `carrel consume` (picking up [papers from Downloads](/docs/guide/adding-papers.md#from-your-downloads)) can run as services that start at login:

```sh
carrel service install web        # http://127.0.0.1:7700; `carrel web --get-url` prints the link to open
carrel service install consume    # whenever a PDF lands in Downloads, and hourly
carrel service                    # what runs
carrel service remove web
```

On Linux these are systemd user units in `~/.config/systemd/user/`; `journalctl --user -u carrel-web` shows their output. On macOS they are launchd agents in `~/Library/LaunchAgents/`, which log to `~/Library/Logs/carrel-web.log` and `carrel-consume.log`. They take `PATH`, `CARREL_HOME`, `CARREL_DOCLING`, `CARREL_EXTRACTOR` and `OLLAMA_HOST` from the shell you install them from, so they find uv or Docling as you do. Run `install` again after changing those; tokens belong in the [config](/docs/reference/configuration.md#api-tokens).

With Nix, Home Manager's module does the same, as systemd units on Linux and launchd agents on macOS:

```nix
imports = [ inputs.carrel.homeManagerModules.default ];
services.carrel = {
  web.enable = true;
  consume.enable = true;
  # docling.package = pkgs.docling;
};
```

`carrel service` leaves the module's units alone.

## Connecting your AI

```sh
carrel integrate claude    # or opencode, codex, gemini, cursor; without a name, which have carrel
```

See [Working with an AI](/docs/guide/ai.md).
