# The web UI

`carrel web` shows your library in the browser: search it, read the PDFs, look through figures and references, write notes, decide on drafts, and empty your Downloads into it.

```sh
carrel web
```

It prints a link like `http://127.0.0.1:7700/?token=…` and opens it. The browser keeps a cookie from that link, so later `http://127.0.0.1:7700` is enough, also after a restart. Stop it with `Ctrl-C`. `carrel web --get-url` prints that link again without starting anything, for a second browser or when carrel runs as a service.

It is for this machine only, like the [TUI](tui.md): it works on your local library and listens on `127.0.0.1`. To share a library with others, [sync it](sharing.md) or [run a server](server.md).

## What it has

- **Library**: all papers or one project's, newest first or by year or title. Search as you type, by words (with the same `author:`, `year:`, `keywords:` filters as `carrel search`) or by [meaning](semantic-search.md). `/` or `Ctrl-K` jumps to the search, `j`/`k` move through the list and `Enter` opens a paper, as in the TUI. *.bib* saves the BibTeX of the papers shown (the project's, or all).
- **A paper**: tabs for the overview (summary and abstract, with keywords, projects and supplementary files beside them), your notes, the extracted text with its figures, the figures alone, its references, the papers citing it, and its BibTeX. Keywords and projects change with a click; notes are Markdown, saved with `Ctrl-S`.
- **Connections** (a paper's tab): its [citation tree](citations.md) among library papers, what it cites or what cites it a few levels deep; the papers most alike by their [embeddings](semantic-search.md); the papers sharing its authors; and *Show on the map*.
- **Reading**: *Read* opens the PDF over the whole page next to the sidebar, in the browser's own reader, with your notes beside it if you want them.
- **Adding**: by DOI, arXiv id, URL or title, or by dropping PDFs anywhere on the page. A paper without a PDF takes one dropped on its page, or downloads it from arXiv.
- **Inbox**: the new PDFs in your [Downloads](adding-papers.md#from-your-downloads), as `carrel consume` would offer them, to add or skip.
- **Drafts**: what an AI proposed, next to what it would change, to approve or reject.
- **Discover**: the papers your library, or a project, cites most but doesn't have.
- **Map**: the library as a network. Papers are dots, coloured by their project or by year and bigger the more links they have. Choose which links to draw; the choice is remembered, and only citations are on at first:
  - **citations**, blue arrows from the citing paper to the cited one;
  - **authors**, orange, thicker the more authors two papers share;
  - **institutions**, lavender squares for the institutions two or more papers come from, linked to their papers ([from OpenAlex](citations.md#institutions));
  - **alike**, green and dashed, the papers most alike by their embeddings, above a threshold you set.

  Two papers linked several ways get their links side by side, so a citation between papers by the same people shows next to their author link, and hovering it says it is a self-citation. Hover a line to see who the shared authors are or who worked where; click a paper to light up its neighbours; double-click to open it. Scroll zooms, dragging moves.
- **People**: everyone who wrote a library paper, most papers first: their spellings, papers and co-authors, and anyone who might be the same person, to confirm as one person or as different people ([who is who](citations.md#who-is-who)). Proposals about people from an AI wait in *Drafts*.
- **Health**: what [`carrel doctor`](maintenance.md) finds, with the command that fixes each, and pairs of papers so alike by their embeddings that one may be a copy of the other.
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
| citation tree | ✓ | ✓ |
| people and who is who | ✓ | ✓ |
| the library as a network | `carrel graph --dot` | ✓ map |
| papers most alike, likely duplicates | | ✓ |
| library health | `carrel doctor` | ✓ |
| drafts | ✓ | ✓ |
| Downloads inbox | `carrel consume` | ✓ |
| sync | ✓ | ✓ |

## Keeping it running

With [Home Manager](https://github.com/nix-community/home-manager), carrel's module runs it as a user service:

```nix
imports = [ inputs.carrel.homeManagerModules.default ];
services.carrel.web.enable = true;   # http://127.0.0.1:7700
```

`carrel web --get-url` prints the link with the token for the first visit, without starting a second server (pass the same `--listen` if you changed it).

## Safety

Any website you visit can make your browser send requests to `localhost`, so carrel doesn't trust a request just because it comes from this machine:

- it answers only to requests addressed to `127.0.0.1`, `localhost` or `[::1]`, which stops websites that point their own name at your machine (DNS rebinding);
- everything under `/api` needs the token, as the cookie the printed link sets, so other websites can't read your library;
- the cookie isn't sent along with other sites' requests (`SameSite=Strict`), and changes coming from another site's page are refused;
- the page can't post forms, be framed by another page or run scripts other than its own (a Content-Security-Policy), and paper texts, notes and AI drafts are shown without forms, buttons or styles of their own, so a draft can't dress up as the page.

On a machine you share, the port is open to every user on it, and browsers send a cookie for `127.0.0.1` to any port there, so the cookie's isolation doesn't keep other users out: only not knowing the token does. The token file is readable by you alone, but don't run `carrel web` where you wouldn't trust the other users with that token.

The token is kept in `~/.local/state/carrel/web-token`. Delete that file and restart `carrel web` to log every browser out.

## The API

The UI uses a JSON API under `/api`, which scripts can use too, with the cookie (`curl -b "carrel-7700=$(cat ~/.local/state/carrel/web-token)" http://127.0.0.1:7700/api/papers`). `src/web.rs` lists its calls; the main ones are `GET /api/papers`, `GET /api/search?q=…&semantic=1`, `GET /api/papers/<id>` and its `pdf`, `fulltext`, `references`, `tree` and `similar`, `GET /api/map`, `GET /api/people`, `GET /api/bib?project=…` and `POST /api/add`.
