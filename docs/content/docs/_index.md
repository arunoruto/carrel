---
title: "Introduction"
weight: 0
---

**A research paper library for you and your AI.** Add papers by PDF, DOI, arXiv id, ISBN or title. carrel finds their metadata, turns each PDF into text and figures an AI can read, keeps a `.bib` file for every paper, and lets an AI search, read and summarize your library, with you approving what it writes.

*A carrel is the private study desk in a university library.*

## What it does

- **Add papers by PDF, DOI, arXiv id, ISBN or title**, or import a `.bib` from a LaTeX project, Zotero or JabRef. Metadata comes from doi.org, Crossref, Semantic Scholar and arXiv; arXiv PDFs are downloaded.
- **Plain files you own.** Each paper is a folder named by its citation key, holding `<key>.bib`, `<key>.pdf`, the extracted text and figures, your notes and supplementary material. Sync it, put it in git, edit it by hand.
- **Readable by AI.** Every PDF becomes `fulltext.md` plus `figures/`, so an assistant can read papers without PDF tools of its own.
- **AI writes drafts, you decide.** Summaries, keywords and notes an AI proposes wait in `drafts/` until you approve them.
- **Projects and bibliographies.** Group papers into projects, keep them in sync with the citations in your LaTeX or Typst files, and get each project's `.bib` in one command.
- **Search everything** with a full-text index over titles, authors, keywords, abstracts, summaries, notes and full texts, with field filters like `author:` and `year:`.
- **Search by meaning** with [semantic search](/docs/guide/semantic-search.md): embeddings from Ollama find papers that say the same in other words or languages.
- **Papers from your Downloads** move into the library with [`carrel consume`](/docs/guide/adding-papers.md#from-your-downloads), asking first or on their own, with an email about what happened.
- **Follow citations**: the references and the citing papers of any paper, marked when they are already in your library.
- **A [web UI](/docs/guide/web.md), a TUI, a CLI and an MCP server** on one core library: read PDFs in the browser, or stay in the terminal.

## How it fits together

```text
        you                         your AI assistant
         │                                  │
   TUI · CLI · web UI                MCP server (carrel mcp)
         └──────────────┬───────────────────┘
                        │
                 carrel library  ── doi.org, Crossref, Semantic Scholar, arXiv
                        │
   ~/Documents/carrel/<key>/<key>.bib, <key>.pdf, fulltext.md, figures/, summary.md, notes.md, drafts/
```

Start with [Installation](/docs/installation.md) and the [Quick start](/docs/quick-start.md).
