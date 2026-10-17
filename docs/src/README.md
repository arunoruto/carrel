# carrel

<div class="hero">

**A research paper library for you and your AI.**

Add papers by PDF, DOI, arXiv id or title. carrel finds their metadata, turns each PDF into text and figures an AI can read, keeps a `.bib` file for every paper, and lets an AI search, read and summarize your library, with you approving what it writes.

`carrel add 2006.11239` · `carrel search diffusion author:ho` · `carrel bib -p thesis > refs.bib`

</div>

*A carrel is the private study desk in a university library.*

## What it does

- **Add papers by PDF, DOI, arXiv id or title**, or import a `.bib` from a LaTeX project, Zotero or JabRef. Metadata comes from doi.org, Crossref, Semantic Scholar and arXiv; arXiv PDFs are downloaded.
- **Plain files you own.** Each paper is a folder named by its citation key, holding `<key>.bib`, `<key>.pdf`, the extracted text and figures, your notes and supplementary material. Sync it, put it in git, edit it by hand.
- **Readable by AI.** Every PDF becomes `fulltext.md` plus `figures/`, so an assistant can read papers without PDF tools of its own.
- **AI writes drafts, you decide.** Summaries, keywords and notes an AI proposes wait in `drafts/` until you approve them.
- **Projects and bibliographies.** Group papers into projects, keep them in sync with the citations in your LaTeX or Typst files, and get each project's `.bib` in one command.
- **Search everything** with a full-text index over titles, authors, keywords, abstracts, summaries, notes and full texts, with field filters like `author:` and `year:`.
- **Follow citations**: the references and the citing papers of any paper, marked when they are already in your library.
- **A TUI, a CLI and an MCP server** on one core library, so other frontends can follow.

## How it fits together

```text
        you                         your AI assistant
         │                                  │
   TUI · CLI                         MCP server (carrel mcp)
         └──────────────┬───────────────────┘
                        │
                 carrel library  ── doi.org, Crossref, Semantic Scholar, arXiv
                        │
   ~/Documents/carrel/<key>/<key>.bib, <key>.pdf, fulltext.md, figures/, summary.md, notes.md, drafts/
```

Start with [Installation](installation.md) and the [Quick start](quick-start.md).
