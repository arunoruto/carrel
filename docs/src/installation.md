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

The Nix package comes with its text extractor (Python with pymupdf4llm from nixpkgs), so nothing is downloaded at runtime and extraction works offline.

## With Cargo

```sh
cargo install --git https://github.com/arunoruto/carrel
```

## The text extractor

Turning PDFs into text and figures is done by an external command (see [Configuration](configuration.md#the-extractor)). Outside of Nix the default runs [pymupdf4llm](https://pypi.org/project/pymupdf4llm/) through [uv](https://docs.astral.sh/uv/), so **install uv**; it fetches pymupdf4llm on first use. Without it, papers are still added, just without their text.
