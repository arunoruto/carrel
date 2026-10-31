---
title: "Clients and servers"
weight: 12
aliases: ["/architecture.html"]
---

There is one program. `carrel` on your laptop is a **client**; `carrel serve` on a machine that never sleeps is a **server**. Both keep a full library of their own, as plain files, and neither talks to the other directly: they meet at the **remote**, the bucket (or folder) that `carrel sync` copies files to and from.

```text
   laptop A                laptop B                       server (carrel serve)
   carrel · TUI · web      carrel · TUI · web                  rounds every few minutes
   ~/Documents/carrel      ~/Documents/carrel             /library
        │                       │                              │
        │ carrel sync           │ carrel sync                  │ sync each round
        └───────────────┬───────┴──────────────────────────────┘
                        │  S3 over HTTPS (or a shared folder)
                  ┌─────┴─────┐
                  │  remote   │  Garage, MinIO, AWS: files only, no logic
                  └───────────┘

   you     ── carrel web (127.0.0.1) ──▶ the laptop's library
   your AI ── carrel mcp (stdio) ──▶ the laptop's library
   any AI  ── HTTPS /mcp + token ──▶ the server's library
```

## What lives where

| | What it is | Where |
|---|---|---|
| **Library** | the truth: a folder per paper with its `.bib`, PDF, `fulltext.md`, figures, notes, drafts, `embeddings.json` | on every client and on the server |
| **Index** | a SQLite cache of the library for search, rebuilt from the files whenever they change | on each machine, in `~/.cache/carrel`; never synced |
| **Remote** | a copy of the library's files that everyone syncs with | a bucket or folder; see [Sharing a library](/docs/guide/sharing.md) |
| **Config** | where things are, tokens, what to do | `~/.config/carrel/config.toml` on each machine; never synced |

Since the remote only stores files, any S3 storage works, and it never needs upgrading. Deleting the index costs nothing; carrel rebuilds it.

## Who does what

| Job | Client | Server |
|---|---|---|
| add papers, attach PDFs, edit, rename | yes, you do | through an AI's MCP tools only as drafts |
| approve or reject drafts | yes | no, a person decides |
| extract text and figures | when a PDF is added | for every PDF still without `fulltext.md` |
| look up references | `carrel refs` | for every paper still without `references.json` |
| compute embeddings | `carrel embed`, with `[embeddings]` | for every paper not yet embedded with its model, with `[embeddings]` |
| pick up new PDFs (`[consume]`) | from your Downloads, asking first, or watched by the [Home Manager module](/docs/guide/adding-papers.md#watching-the-folder) | from a drop folder, only with `auto = true` |
| notify | about your own Downloads | about its own trouble (Ollama unreachable), never about others' papers |
| search | its own index | its own index, for the AIs using it |

So the slow work happens once, on the server, and everyone gets the result with their next `carrel sync`: a PDF you add on your laptop is extracted, looked up and embedded on the server, and its `fulltext.md`, `references.json` and `embeddings.json` come back to every client.

A server is optional. Without one, each client does all of this itself, and sync alone shares the library.

## How they connect

- **Client or server ↔ remote:** `carrel sync`, over the S3 API (HTTPS) with the bucket's key, or file copies to a folder. Each side remembers what both held at the last sync, in `.carrel/sync.json`, so changes go the right way and conflicts are caught: see [Sharing a library](/docs/guide/sharing.md#through-a-bucket). The server syncs at the start of each round and again when the round changed anything.
- **Browser ↔ client:** `carrel web` serves the [web UI](/docs/guide/web.md) and its JSON API on `127.0.0.1` only, guarded by a cookie from the link it prints. The server has no web UI (yet).
- **AI ↔ client:** `carrel mcp`, started by the AI app, over stdin and stdout. Nothing is listening on the network. See [Working with an AI](/docs/guide/ai.md).
- **AI ↔ server:** MCP over HTTP at `https://<server>/mcp`, with the token in an `Authorization: Bearer` header, or without one from a [trusted network](/docs/guide/server.md#networks-without-a-token). Put the server behind a reverse proxy with HTTPS.
- **Both ↔ the internet:** metadata from doi.org, Crossref, Semantic Scholar, arXiv, OpenAlex and ADS; embeddings from the Ollama named in `[embeddings]`; mail through the server in `[notify.email]`.

## When two sides change the same thing

Each paper is its own folder, so people adding papers never collide. The server only writes files that are missing (a `fulltext.md`, a `references.json`) or out of date (`embeddings.json`), and drafts, which a person approves later. A file changed on both sides between two syncs is a conflict, which carrel leaves alone until `carrel sync --keep local` or `--keep remote` settles it.
