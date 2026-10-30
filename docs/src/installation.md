# Installation

carrel runs on Linux (x86_64 and arm64) and macOS (Apple silicon).

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

The Nix package comes with its text extractor (Python with pymupdf4llm from nixpkgs), so nothing is downloaded at runtime and extraction works offline. For [Docling](configuration.md#the-extractor) instead, add `pkgs.docling` to your packages.

## Docker

For a server, the image `ghcr.io/arunoruto/carrel` runs [`carrel serve`](server.md) and comes with the extractor:

```sh
docker run --rm -it -v ~/Documents/carrel:/library ghcr.io/arunoruto/carrel:latest list
```

## With Cargo

```sh
cargo install --git https://github.com/arunoruto/carrel
```

This carrel has no [web UI](web.md): `carrel web` serves only its API. The binaries, Nix and Docker have it. To build it in yourself, with [Node.js](https://nodejs.org):

```sh
git clone https://github.com/arunoruto/carrel && cd carrel
(cd web && npm ci && npm run build)
cargo install --path .
```

## The text extractor

Turning PDFs into text and figures is done by an external program (see [Configuration](configuration.md#the-extractor)): [Docling](https://github.com/docling-project/docling) if it is installed, else [pymupdf4llm](https://pypi.org/project/pymupdf4llm/). Outside of Nix and Docker, pymupdf4llm runs through [uv](https://docs.astral.sh/uv/), so **install uv** (or Docling); uv fetches pymupdf4llm on first use. Without either, papers are still added, just without their text.
