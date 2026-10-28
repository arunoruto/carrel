# Running a server

`carrel serve` runs the library on a server, for a group and its AIs:

- it serves the [MCP tools](mcp-tools.md) over HTTP at `/mcp`, so an AI anywhere on the network can use the library;
- every few minutes it [syncs](sharing.md) with the remote and does the slow work once, for everyone: it extracts the text and figures of new PDFs and looks up the references of new papers. Everyone else gets the results with their next `carrel sync`.

Everyone keeps their own library and syncs it as before; the server is one more member of the group, one that never sleeps. It needs a [remote](sharing.md) in its config to share its work.

## With Docker

The image is `ghcr.io/arunoruto/carrel`: `latest` (the latest release, also `0.2.0`, `0.2`, ...) or `edge` (main). It comes with the extractor.

```yaml
# compose.yaml
services:
  carrel:
    image: ghcr.io/arunoruto/carrel:latest
    restart: unless-stopped
    ports: ["8080:8080"]
    environment:
      CARREL_SERVE_TOKEN: ${CARREL_SERVE_TOKEN}       # what clients must send
      AWS_SECRET_ACCESS_KEY: ${AWS_SECRET_ACCESS_KEY} # the bucket's secret
      ADS_API_TOKEN: ${ADS_API_TOKEN}                 # optional
    volumes:
      - ./config.toml:/config/carrel/config.toml:ro
      - library:/library
      - cache:/cache
volumes:
  library:
  cache:
```

with the secrets in `.env` next to it, and in `config.toml`:

```toml
[remote]
type = "s3"
endpoint = "https://s3.example.org"
region = "garage"
bucket = "papers"
access_key = "GK..."

[serve]
every = 5   # minutes between rounds
```

The image runs as user 1000. If the library is a folder on the host rather than a volume, that user must be able to write it.

## On NixOS

The flake has a module:

```nix
{
  inputs.carrel.url = "github:arunoruto/carrel";

  outputs = { nixpkgs, carrel, ... }: {
    nixosConfigurations.server = nixpkgs.lib.nixosSystem {
      modules = [
        carrel.nixosModules.default
        {
          services.carrel = {
            enable = true;
            listen = "0.0.0.0:8080";
            environmentFile = "/run/secrets/carrel.env"; # CARREL_SERVE_TOKEN=..., AWS_SECRET_ACCESS_KEY=...
            settings.remote = {
              type = "s3";
              endpoint = "https://s3.example.org";
              region = "garage";
              bucket = "papers";
              access_key = "GK...";
            };
          };
        }
      ];
    };
  };
}
```

The library lives in `/var/lib/carrel/library`. `settings` is the [config](configuration.md) as Nix; keep secrets in `environmentFile`, since `settings` ends up in the Nix store.

## Connecting an AI

Clients send the token in a header. For Claude Code:

```sh
claude mcp add --transport http carrel https://papers.example.org/mcp \
  --header "Authorization: Bearer $CARREL_SERVE_TOKEN"
```

Other MCP clients take the same URL and header; in Python, the `mcp` package's `streamablehttp_client(url, headers={...})`.

## Security

Without a token, `carrel serve` only listens on `127.0.0.1`: the library holds publishers' PDFs. Anything reachable from the network needs `CARREL_SERVE_TOKEN` (or `[serve] token` in the config, written out or `{ file = "..." }`), and should sit behind a reverse proxy with HTTPS (Caddy, nginx, Traefik) so the token never crosses the network in the clear. `/health` answers without a token, for health checks.
