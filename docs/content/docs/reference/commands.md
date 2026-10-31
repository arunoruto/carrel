---
title: "Commands"
weight: 1
aliases: ["/commands.html"]
---

Run `carrel <command> --help` for every option.

| Command | What it does |
|---|---|
| `carrel` | the config's `default`: the [TUI](/docs/guide/tui.md) unless set |
| `tui` | the [TUI](/docs/guide/tui.md) |
| `web [--listen addr] [--no-open] [--get-url]` | the [web UI](/docs/guide/web.md), on this machine; `--get-url` only prints the login link |
| `add <source>…` | add papers by PDF, DOI, arXiv id/URL, ISBN, ADS bibcode/URL or exact title |
| `attach <id> [pdf]` | give a paper its PDF; without a file, from arXiv |
| `addto <id> <file or URL>…` | add supplementary files |
| `import <file.bib> [-p project] [--pdfs]` | import a `.bib`, keeping its keys; [from JabRef](/docs/guide/adding-papers.md#from-jabref) with its files and groups |
| `import-ilibrarian <data folder>` | move an [I, Librarian](/docs/guide/adding-papers.md#moving-from-i-librarian) library over |
| `list [-p project] [--json]` | all papers, or a project's |
| `search <query>…` | full-text search, with `field:` and `project:` filters |
| `search -s <query>…` | [semantic search](/docs/guide/semantic-search.md): by meaning |
| `embed [ids…] [--all]` | compute the embeddings for semantic search |
| `consume [folders…] [--yes] [--again] [--days n]` | move new PDFs from [Downloads](/docs/guide/adding-papers.md#from-your-downloads) into the library |
| `notify [message]` | try the [notifiers](/docs/reference/configuration.md#notifications) |
| `show <id> [--json]` | a paper's details |
| `open <id>` | open its PDF |
| `edit <id>` | edit its `.bib` |
| `rename <id> [new-id]` | change its key everywhere; without a new id, from the key template |
| `update <id> [source]` / `update --all` | fetch metadata again |
| `refs <id> [--cited-by] [--refresh]` / `refs --all` | its [references](/docs/guide/citations.md), or the papers citing it; `--all` also looks up where the authors worked, for the map |
| `graph [id] [-p project] [--dot] [--json]` | how a paper connects to the library, or what the library cites most but lacks |
| `authors [name] [--json]` | the people behind the papers: spellings, papers, co-authors ([who is who](/docs/guide/citations.md#who-is-who)) |
| `authors same\|different <spelling>…` | confirm that spellings are one person, or different people |
| `authors approve\|reject <draft>` | decide on an AI's proposal about people |
| `tree <id> [--cited-by] [-d depth]` | the citation tree among library papers |
| `extract [ids…] [--all]` | run the text extraction again |
| `drafts` | AI proposals waiting for review |
| `approve <id> <summary\|keywords\|notes\|references>` | accept a draft |
| `reject <id> <summary\|keywords\|notes\|references>` | discard a draft |
| `project [list\|show\|add\|remove\|delete\|sync]` | [projects](/docs/guide/projects.md) |
| `bib [ids…] [-p project] [--jabref]` | BibTeX for papers, a project, or the library; `--jabref` adds [file links and projects as groups](/docs/guide/projects.md#jabref) |
| `doctor [--all] [--online] [--json]` | [check the library](/docs/guide/maintenance.md) |
| `rm <id>` | delete a paper and its files |
| `sync [-n] [--keep local\|remote]` | [share the library](/docs/guide/sharing.md) through the config's remote |
| `serve [--listen addr] [--every minutes]` | [run the library on a server](/docs/guide/server.md): MCP over HTTP, syncing and background work |
| `config` | where the config, library and index are |
| `mcp` | the [MCP server](/docs/guide/ai.md) on stdin/stdout |
| `integrate [agent] [--remove]` | [connect an AI agent](/docs/guide/ai.md#connecting): claude, opencode, codex, gemini, cursor |
| `service [install\|remove web\|consume]` | [run in the background](/docs/installation.md#running-in-the-background): systemd on Linux, launchd on macOS |
