# Configuration

Settings live in `~/.config/carrel/config.toml` (or `$XDG_CONFIG_HOME/carrel/config.toml`), on Linux and macOS alike; every one is optional. `carrel config` shows the paths in use.

```toml
library = "~/Documents/carrel"
key = "{{ author | lower }}{{ year }}{{ word | lower }}"
# extractor = "..."
# default = "tui"
# theme = "catppuccin-mocha"

# [tokens]   API tokens, see below
# [remote]   where the library is shared, see Sharing a library
# [serve]    carrel serve's settings, see Running a server
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

`extractor` is the shell command that turns a PDF into text and figures. It runs in an empty folder with the PDF's path in `$PDF` and must write `fulltext.md` there, and optionally figure images into `figures/`. `CARREL_EXTRACTOR` overrides it.

The default runs pymupdf4llm through uv; the Nix package uses pymupdf4llm from nixpkgs instead. A wrapper around [docling](https://github.com/docling-project/docling) or [marker](https://github.com/datalab-to/marker) works too, for instance.

## The server

`[serve]` holds [`carrel serve`](server.md)'s settings; its flags override them:

```toml
[serve]
listen = "0.0.0.0:8080"   # 127.0.0.1:8080 by default
every = 5                 # minutes between rounds of syncing and background work
token = { file = "/run/secrets/carrel-token" }   # what clients must send; CARREL_SERVE_TOKEN overrides it
```

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
| `CARREL_EXTRACTOR` | the extractor command |
| `S2_API_KEY` | a free [Semantic Scholar API key](https://www.semanticscholar.org/product/api#api-key); without one, references, citations and some lookups are often rate-limited |
| `ADS_API_TOKEN` (or `ADS_DEV_KEY`) | a free [NASA ADS token](https://ui.adsabs.harvard.edu/user/settings/token); turns on ADS as the first [reference source](citations.md) |
| `OPENALEX_API_KEY` | a free [OpenAlex key](https://openalex.org/settings/api); without one, everyone on your network shares about 1000 requests a day |
| `AWS_ENDPOINT_URL`, `AWS_REGION`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` | override an S3 [remote](sharing.md)'s settings |
| `CARREL_SERVE_TOKEN` | the token [`carrel serve`](server.md)'s clients must send |
| `VISUAL`, `EDITOR` | the editor for `.bib` files and notes |
