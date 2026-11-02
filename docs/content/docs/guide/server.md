---
title: "Running a server"
weight: 12
aliases: ["/server.html"]
---

`carrel serve` runs the library on a server, for a group and its AIs:

- it serves the [MCP tools](/docs/reference/mcp-tools.md) over HTTP at `/mcp`, so an AI anywhere on the network can use the library;
- it serves the [web UI](/docs/guide/web.md) at its address, so the group can read and add papers in a browser without installing anything;
- every few minutes it [syncs](/docs/guide/sharing.md) with the remote and does the slow work once, for everyone: it extracts the text and figures of new PDFs, looks up the references of new papers and, with `[embeddings]` in its config, computes their [embeddings](/docs/guide/semantic-search.md). Everyone else gets the results with their next `carrel sync`.
- with `[consume]` and `auto = true` in its config, it adds the papers that turn up in a folder (a shared drop folder, say) and logs what it added;
- with [notifiers](/docs/reference/configuration.md#notifications) in its config, it tells you about its own trouble, such as Ollama going away. Papers others drop in don't land in your inbox; only a `carrel consume` on your own machine tells you about your Downloads.

Everyone keeps their own library and syncs it as before; the server is one more member of the group, one that never sleeps. It needs a [remote](/docs/guide/sharing.md) in its config to share its work. [Clients and servers](/docs/guide/architecture.md) shows how the pieces connect.

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

The library lives in `/var/lib/carrel/library`. `settings` is the [config](/docs/reference/configuration.md) as Nix; keep secrets in `environmentFile`, since `settings` ends up in the Nix store.

## The web UI

The server's address opens the [web UI](/docs/guide/web.md), the same as `carrel web` on your own machine: papers, the reader, adding by DOI or by dropping PDFs, Discover, the map. Papers added there are extracted by the server and reach everyone with its next sync. Text extraction needs an extractor on the server: the image has PyMuPDF, and `CARREL_DOCLING` (or `docling` in its config) can point it at a [docling-serve](/docs/reference/configuration.md#the-extractor).

### Logging in

With a login provider that speaks OpenID Connect ([pocket-id](https://pocket-id.org), Keycloak, Authentik, ...), people log in with their accounts there:

```toml
[serve]
url = "https://papers.example.org"          # where browsers reach carrel

[serve.oidc]
issuer = "https://id.example.org"
client_id = "..."
client_secret = { file = "/run/secrets/carrel-oidc" }
```

or `CARREL_OIDC_ISSUER`, `CARREL_OIDC_CLIENT_ID` and `CARREL_OIDC_SECRET` in a container. At the provider, make a client with the callback `https://papers.example.org/auth/callback`; carrel asks for the scopes `openid profile email groups`. Two groups there decide what people may do:

| who | may |
|---|---|
| anyone from a [trusted network](#networks-without-a-token), without logging in | read, and add papers and files |
| group `carrel` | also change things: notes, metadata, drafts, projects, people |
| group `admin` | also delete papers |

Someone in neither group can't log in. A login lasts 30 days. Members make **tokens** for their machines on the web UI's Tokens page, for the [AI](#connecting-an-ai) and other clients that can't log in through a browser; a token acts for the person who made it, with their groups at the time, until they revoke it. The server's own token (`CARREL_SERVE_TOKEN`) counts as an admin's.

Sessions and tokens are kept in `.carrel/` in the library, only as hashes. carrel asks the provider who logged in, over HTTPS from the server, rather than reading it from the browser.

### Without a login provider

Then the server prints a link with a token of its own when it starts:

```
12:00:00 web UI: https://papers.example.org/?token=4f1c…
```

(`docker logs carrel | grep "web UI"` finds it.) Opening it logs a browser in as an admin, for a year; trusted networks may still read and add without it. The token is kept in `.carrel/web-token`; delete that file and restart to change it, which logs everyone out. Set `url` in `[serve]` (or `CARREL_SERVE_URL`) to the address browsers use, so the link is one to click; the server then also answers to that name only.

## Connecting an AI

Clients send a token in a header: a member's own from the Tokens page, or the server's. For Claude Code:

```sh
claude mcp add --transport http carrel https://papers.example.org/mcp \
  --header "Authorization: Bearer $CARREL_SERVE_TOKEN"
```

Other MCP clients take the same URL and header; in Python, the `mcp` package's `streamablehttp_client(url, headers={...})`.

## Security

Without a token, `carrel serve` only listens on `127.0.0.1`: the library holds publishers' PDFs. Anything reachable from the network needs `CARREL_SERVE_TOKEN` (or `[serve] token` in the config, written out or `{ file = "..." }`), and should sit behind a reverse proxy with HTTPS (Caddy, nginx, Traefik) so the token never crosses the network in the clear. `/health` answers without a token, for health checks.

### Networks without a token

Clients on networks you trust can skip the token, for example the server itself and your Tailscale network:

```toml
[serve]
listen = "0.0.0.0:8080"
trusted = ["127.0.0.0/8", "::1", "100.64.0.0/10", "fd7a:115c:a1e0::/48"]
```

Entries are networks (`100.64.0.0/10`) or single addresses (`10.0.0.5`). `100.64.0.0/10` and `fd7a:115c:a1e0::/48` are the addresses Tailscale gives its devices. Everyone else still needs the token. Without a token set, nobody else gets in.

carrel checks the address the connection comes from, not the name it was reached by. So watch for two traps:

- **A reverse proxy** makes every request it forwards come from the proxy. Trusting the proxy's address, such as `127.0.0.0/8` for a proxy on the same machine, then trusts everyone who can reach the proxy. Name the proxy in `proxies` instead. carrel then uses the address the proxy saw, which the proxy adds to `X-Forwarded-For`:

  ```toml
  [serve]
  trusted = ["100.64.0.0/10"]
  proxies = ["172.18.0.0/16"]   # here the Docker network Traefik reaches carrel through
  ```

  carrel only reads the last `X-Forwarded-For` entry, the one each proxy in `proxies` added. Entries further left came from the client and can be faked. Anything that can reach carrel from a network in `proxies` can claim any address, so keep that list to the proxies themselves.
- **In Docker**, a request from the host itself comes from the Docker bridge (often `172.17.0.1`), not `127.0.0.1`. Requests from other machines, Tailscale included, keep their address.
