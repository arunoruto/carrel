# Commands

Run `carrel <command> --help` for every option.

| Command | What it does |
|---|---|
| `carrel` | the [TUI](tui.md) |
| `add <source>…` | add papers by PDF, DOI, arXiv id/URL or exact title |
| `attach <id> [pdf]` | give a paper its PDF; without a file, from arXiv |
| `addto <id> <file or URL>…` | add supplementary files |
| `import <file.bib> [-p project] [--pdfs]` | import a `.bib`, keeping its keys |
| `list [-p project] [--json]` | all papers, or a project's |
| `search <query>…` | full-text search, with `field:` and `project:` filters |
| `show <id> [--json]` | a paper's details |
| `open <id>` | open its PDF |
| `edit <id>` | edit its `.bib` |
| `rename <id> [new-id]` | change its key everywhere; without a new id, from the key template |
| `update <id> [source]` / `update --all` | fetch metadata again |
| `refs <id> [--cited-by]` | its references, or the papers citing it |
| `extract [ids…] [--all]` | run the text extraction again |
| `drafts` | AI proposals waiting for review |
| `approve <id> <summary\|keywords\|notes>` | accept a draft |
| `reject <id> <summary\|keywords\|notes>` | discard a draft |
| `project [list\|show\|add\|remove\|delete\|sync]` | [projects](projects.md) |
| `bib [ids…] [-p project]` | BibTeX for papers, a project, or the library |
| `doctor [--all] [--json]` | [check the library](maintenance.md) |
| `rm <id>` | delete a paper and its files |
| `config` | where the config, library and index are |
| `mcp` | the [MCP server](ai.md) on stdin/stdout |
