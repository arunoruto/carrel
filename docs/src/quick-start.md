# Quick start

Add a few papers:

```sh
carrel add 1706.03762                              # an arXiv id: metadata and PDF
carrel add 10.1038/nature14539                     # a DOI: metadata
carrel add ~/Downloads/some-paper.pdf              # a PDF: its DOI or title is looked up
carrel add "Deep Residual Learning for Image Recognition"
```

Each becomes a folder in `~/Documents/carrel` named by its citation key, such as `vaswani2017attention`.

Look around:

```sh
carrel list
carrel search attention author:vaswani
carrel show vaswani2017attention
carrel                                             # the TUI
```

Connect your AI assistant (here Claude Code; see [Working with an AI](ai.md) for others):

```sh
claude mcp add --scope user carrel -- carrel mcp
```

and ask it: *"Read the papers in my library that have no summary yet and propose summaries and keywords."* Its proposals arrive as drafts:

```sh
carrel drafts
carrel approve vaswani2017attention summary        # or reject; the TUI shows them side by side (d)
```

Write a paper with them:

```sh
carrel project sync thesis ~/thesis/*.tex          # the papers your LaTeX cites
carrel bib -p thesis > ~/thesis/refs.bib
```
