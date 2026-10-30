# The web UI

`carrel web` shows your library in the browser: search it, read the PDFs, look through figures and references, write notes, decide on drafts, and empty your Downloads into it.

```sh
carrel web
```

It prints a link like `http://127.0.0.1:7700/?token=…` and opens it. The browser keeps a cookie from that link, so later `http://127.0.0.1:7700` is enough, also after a restart. Stop it with `Ctrl-C`.

It is for this machine only, like the [TUI](tui.md): it works on your local library and listens on `127.0.0.1`. To share a library with others, [sync it](sharing.md) or [run a server](server.md).

## What it has

- **Library**: all papers or one project's, newest first or by year or title. Search as you type, by words (with the same `author:`, `year:`, `keywords:` filters as `carrel search`) or by [meaning](semantic-search.md). `/` or `Ctrl-K` jumps to the search.
- **A paper**: its PDF next to tabs for the overview (summary, abstract, keywords, projects, supplementary files), your notes, the extracted text with its figures, the figures alone, its references, the papers citing it, and its BibTeX. Keywords and projects change with a click; notes are Markdown, saved with `Ctrl-S`.
- **Adding**: by DOI, arXiv id, URL or title, or by dropping PDFs anywhere on the page. A paper without a PDF takes one dropped on its page, or downloads it from arXiv.
- **Inbox**: the new PDFs in your [Downloads](adding-papers.md#from-your-downloads), as `carrel consume` would offer them, to add or skip.
- **Drafts**: what an AI proposed, next to what it would change, to approve or reject.
- **Discover**: the papers your library, or a project, cites most but doesn't have.
- **Sync** with the remote, a dark and a light theme (Catppuccin, like the TUI), and a layout that works on a phone.

The TUI and the web UI cover the same ground; each has what suits it:

| | TUI | Web |
|---|---|---|
| browse, search by words, filter by project | ✓ | ✓ |
| search by meaning | `carrel search -s` | ✓ |
| read the PDF | opens your PDF viewer | ✓ built in |
| full text and figures | | ✓ |
| notes, keywords, projects | ✓ (in your editor) | ✓ |
| add by DOI, title or file | ✓ | ✓, and drag and drop |
| references, citing papers, adding them | ✓ | ✓ |
| citation tree, people and who is who | ✓ | not yet |
| drafts | ✓ | ✓ |
| Downloads inbox | `carrel consume` | ✓ |
| sync | ✓ | ✓ |

## Keeping it running

With [Home Manager](https://github.com/nix-community/home-manager), carrel's module runs it as a user service:

```nix
imports = [ inputs.carrel.homeManagerModules.default ];
services.carrel.web.enable = true;   # http://127.0.0.1:7700
```

`journalctl --user -u carrel-web` shows the link with the token, for the first visit.

## Safety

Any website you visit can make your browser send requests to `localhost`, so carrel doesn't trust a request just because it comes from this machine:

- it answers only to requests addressed to `127.0.0.1`, `localhost` or `[::1]`, which stops websites that point their own name at your machine (DNS rebinding);
- everything under `/api` needs the cookie that only the printed link sets, so other websites and other users of the machine can't read your library;
- the cookie isn't sent along with other sites' requests (`SameSite=Strict`), and changes coming from another site's page are refused.

The token is kept in `~/.local/state/carrel/web-token`. Delete that file and restart `carrel web` to log every browser out.

## The API

The UI uses a JSON API under `/api`, which scripts can use too, with the cookie (`curl -b "carrel-7700=$(cat ~/.local/state/carrel/web-token)" http://127.0.0.1:7700/api/papers`). `src/web.rs` lists its calls; the main ones are `GET /api/papers`, `GET /api/search?q=…&semantic=1`, `GET /api/papers/<id>` and its `pdf`, `fulltext` and `references`, and `POST /api/add`.
