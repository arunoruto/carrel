# Configuration

Settings live in `~/.config/carrel/config.toml` (or `$XDG_CONFIG_HOME/carrel/config.toml`), on Linux and macOS alike; every one is optional. `carrel config` shows the paths in use.

```toml
library = "~/Documents/carrel"
key = "{{ author | lower }}{{ year }}{{ word | lower }}"
# extractor = "auto"
# docling = "/path/to/docling"
# default = "tui"
# theme = "catppuccin-mocha"

# [tokens]   API tokens, see below
# [remote]   where the library is shared, see Sharing a library
# [serve]    carrel serve's settings, see Running a server
# [embeddings]  where Ollama is, see Semantic search
# [consume]  folders where new papers turn up, see Adding papers
# [notify]   how carrel tells you what happened, see below
```

## What `carrel` alone does

`default` is what running `carrel` without a command does, written as its arguments: `tui` (the default) opens the [TUI](tui.md), `list -p thesis` lists a project, `search` with a query… Arguments are split on spaces; quotes aren't understood.

`theme` picks the TUI's colours ([themes](tui.md#themes)).

## The library

`library` is the library folder. The environment variable `CARREL_HOME` overrides it. The default is `carrel` in your Documents folder.

`[remote]` is where `carrel sync` shares it: an S3 bucket or a folder ([Sharing a library](sharing.md)).

## Citation keys

`key` is a [Jinja](https://docs.rs/minijinja) template for the keys of new papers (and for `carrel rename <id>` without a new id). Its variables, all ASCII:

| Variable | Value |
|---|---|
| `author` | the first author's family name |
| `authors` | all family names |
| `year` | the year |
| `title` | the title |
| `word` | the first meaningful title word (articles and short words in English, German, French, Spanish and Italian are skipped) |
| `words` | all meaningful title words |
| `venue`, `doi`, `arxiv`, `type` | journal or conference, identifiers, entry type |

Missing values are empty. The result keeps ASCII letters, digits and `-_.:`.

For *He, Zhang, Ren and Sun (2016), Deep Residual Learning for Image Recognition*:

| Template | Key |
|---|---|
| `{{ author \| lower }}{{ year }}{{ word \| lower }}` (default) | `he2016deep` |
| `{{ author }}:{{ year }}` | `He:2016` |
| `{{ authors[:2] \| join('-') \| lower }}_{{ year }}` | `he-zhang_2016` |
| `{{ author \| lower }}{{ year }}{{ words[:3] \| join \| lower }}` | `he2016deepresiduallearning` |

## The extractor

`extractor` is what turns a PDF into `fulltext.md` and its figures into `figures/`. `CARREL_EXTRACTOR` overrides it.

| Value | Extractor |
|---|---|
| `auto` (the default) | Docling if it is installed, else PyMuPDF if it can run, else none |
| `docling` | [Docling](https://github.com/docling-project/docling): better tables and reading order, but slow |
| `pymupdf` | [pymupdf4llm](https://pypi.org/project/pymupdf4llm/): fast and good enough for most papers |
| anything else | a shell command, see below |

**PyMuPDF** runs through [uv](https://docs.astral.sh/uv/), which fetches pymupdf4llm on first use; the Nix package and the Docker image bring their own Python with it instead, so for them it is always there.

**Docling** is found as `docling` on your `PATH`, or where `docling` in the config says (useful for services, whose `PATH` is short). Install it with `uv tool install docling` or `pipx install docling`; on Nix it is in nixpkgs, e.g. `home.packages = [ pkgs.docling ];` (it is not part of carrel's package: it brings PyTorch, gigabytes of it), or for the [NixOS module](server.md#on-nixos) `services.carrel.settings.docling = lib.getExe pkgs.docling;`. With `auto`, carrel then uses it. carrel runs `docling --to md --image-export-mode referenced --no-ocr`, without OCR because papers born digital don't need it and it is slow, and moves the result into carrel's layout. Docling downloads its models on first use, so the first paper takes a while, and on a CPU every paper takes tens of seconds to minutes; there is no time limit. It leaves formulas out (`<!-- formula-not-decoded -->`) unless its formula model is on: point `docling` in the config at a script that runs `docling --enrich-formula "$@"` (slower still).

**Nothing installed**: papers and PDFs are still added, without their text; carrel says so once. `carrel config` shows which extractor is in use, and `carrel doctor` lists the papers without text; `carrel extract --all` extracts them once there is an extractor. An extractor asked for by name but not installed is an error.

**A shell command** runs in an empty folder with the PDF's path in `$PDF` and must write `fulltext.md` there, and optionally figure images into `figures/`. A wrapper around [marker](https://github.com/datalab-to/marker) works, for instance.

## The server

`[serve]` holds [`carrel serve`](server.md)'s settings; its flags override them:

```toml
[serve]
listen = "0.0.0.0:8080"   # 127.0.0.1:8080 by default
every = 5                 # minutes between rounds of syncing and background work
token = { file = "/run/secrets/carrel-token" }   # what clients must send; CARREL_SERVE_TOKEN overrides it
trusted = ["127.0.0.0/8", "100.64.0.0/10"]       # networks that need no token, here the server and Tailscale
proxies = ["172.18.0.0/16"]                      # reverse proxies whose X-Forwarded-For carrel believes
```

See [Networks without a token](server.md#networks-without-a-token) before trusting anything behind a reverse proxy.

## Notifications

carrel tells you what it did while you weren't looking: PDFs that `carrel consume` found in your [Downloads](adding-papers.md#from-your-downloads) or added from there when run from a timer, and a [server](server.md) that can't reach Ollama. A server never tells you about papers others dropped in its folder. Use any of these, or several:

```toml
[notify]
desktop = true   # a notification on this computer: over D-Bus on Linux, Notification Center on macOS

[notify.email]   # through a mail server that takes mail without login or TLS, as many university relays do
server = "unimail.tu-dortmund.de:25"   # port 25 when left out
from = "you@tu-dortmund.de"
to = "you@tu-dortmund.de"

[notify.telegram]
token = { file = "~/.config/carrel/telegram-token" }   # or the token written out
chat_id = 123456789                                    # or "@channel" for a public channel
```

For Telegram, make a bot by messaging [@BotFather](https://t.me/BotFather), which gives you its token. Send your bot any message, then open `https://api.telegram.org/bot<token>/getUpdates`: the number after `"chat":{"id":` is your `chat_id`. Messages go as plain text, cut at Telegram's 4096 characters; `api = "https://…"` points at a self-hosted Bot API server instead.

`carrel notify` sends a test message.

## API tokens

Tokens for ADS, Semantic Scholar and OpenAlex can go in the config instead of [environment variables](#environment-variables), either written out or kept in a file of their own (a password manager or agenix/sops can write that file):

```toml
[tokens]
ads = { file = "~/.config/carrel/ads-token" }
s2 = "your-semantic-scholar-key"
# openalex = "..."
```

An environment variable wins over the config. `carrel config` lists the tokens it found, never their values.

## Environment variables

| Variable | Purpose |
|---|---|
| `CARREL_HOME` | the library folder |
| `CARREL_EXTRACTOR` | the [extractor](#the-extractor): `auto`, `docling`, `pymupdf` or a command |
| `S2_API_KEY` | a free [Semantic Scholar API key](https://www.semanticscholar.org/product/api#api-key); without one, references, citations and some lookups are often rate-limited |
| `ADS_API_TOKEN` (or `ADS_DEV_KEY`) | a free [NASA ADS token](https://ui.adsabs.harvard.edu/user/settings/token); turns on ADS as the first [reference source](citations.md) |
| `OPENALEX_API_KEY` | a free [OpenAlex key](https://openalex.org/settings/api); without one, everyone on your network shares about 1000 requests a day |
| `AWS_ENDPOINT_URL`, `AWS_REGION`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` | override an S3 [remote](sharing.md)'s settings |
| `CARREL_SERVE_TOKEN` | the token [`carrel serve`](server.md)'s clients must send |
| `OLLAMA_HOST` | where Ollama is, for [semantic search](semantic-search.md) |
| `VISUAL`, `EDITOR` | the editor for `.bib` files and notes |
