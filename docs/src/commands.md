# Commands

Run `carrel <command> --help` for every option.

| Command | What it does |
|---|---|
| `carrel` | the [TUI](tui.md) |
| `add <source>…` | add papers by PDF, DOI, arXiv id/URL or exact title |
| `attach <id> [pdf]` | give a paper its PDF; without a file, from arXiv |
| `addto <id> <file or URL>…` | add supplementary files |
| `import <file.bib> [-p project] [--pdfs]` | import a `.bib`, keeping its keys |
| `import-ilibrarian <data folder>` | move an [I, Librarian](adding-papers.md#moving-from-i-librarian) library over |
| `list [-p project] [--json]` | all papers, or a project's |
| `search <query>…` | full-text search, with `field:` and `project:` filters |
| `show <id> [--json]` | a paper's details |
| `open <id>` | open its PDF |
| `edit <id>` | edit its `.bib` |
| `rename <id> [new-id]` | change its key everywhere; without a new id, from the key template |
| `update <id> [source]` / `update --all` | fetch metadata again |
| `refs <id> [--cited-by] [--refresh]` / `refs --all` | its [references](citations.md), or the papers citing it |
| `graph [id] [-p project] [--dot] [--json]` | how a paper connects to the library, or what the library cites most but lacks |
| `authors [name] [--json]` | the people behind the papers: spellings, papers, co-authors ([who is who](citations.md#who-is-who)) |
| `authors same\|different <spelling>…` | confirm that spellings are one person, or different people |
| `authors approve\|reject <draft>` | decide on an AI's proposal about people |
| `tree <id> [--cited-by] [-d depth]` | the citation tree among library papers |
| `extract [ids…] [--all]` | run the text extraction again |
| `drafts` | AI proposals waiting for review |
| `approve <id> <summary\|keywords\|notes\|references>` | accept a draft |
| `reject <id> <summary\|keywords\|notes\|references>` | discard a draft |
| `project [list\|show\|add\|remove\|delete\|sync]` | [projects](projects.md) |
| `bib [ids…] [-p project] [--files]` | BibTeX for papers, a project, or the library; `--files` adds the PDFs' paths for JabRef |
| `doctor [--all] [--json]` | [check the library](maintenance.md) |
| `rm <id>` | delete a paper and its files |
| `config` | where the config, library and index are |
| `mcp` | the [MCP server](ai.md) on stdin/stdout |
