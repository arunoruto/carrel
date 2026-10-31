---
title: "Working with an AI"
weight: 5
aliases: ["/ai.html"]
---

`carrel mcp` serves the library over the [Model Context Protocol](https://modelcontextprotocol.io), so assistants such as Claude can search it, read papers and figures, follow citations and propose summaries, keywords and notes.

## Connecting

```sh
carrel integrate claude           # or opencode, codex, gemini, cursor
carrel integrate                  # which agents have carrel
carrel integrate claude --remove
```

For Claude Code this runs `claude mcp add --scope user`. For the others it adds carrel to their config file and keeps everything else in it: `~/.config/opencode/opencode.json`, `~/.codex/config.toml`, `~/.gemini/settings.json` or `~/.cursor/mcp.json`. It leaves a JSON config with comments alone, so add the entry by hand there, and for other clients:

| Client | Entry |
|---|---|
| Claude Code | `claude mcp add --scope user carrel -- carrel mcp` |
| opencode | `"mcp": { "carrel": { "type": "local", "command": ["carrel", "mcp"], "enabled": true } }` |
| Codex | `[mcp_servers.carrel]` with `command = "carrel"` and `args = ["mcp"]` |
| Claude Desktop, Gemini CLI, Cursor and most others | `"mcpServers": { "carrel": { "command": "carrel", "args": ["mcp"] } }` |

For a library on a [server](/docs/guide/server.md#connecting-an-ai), clients connect over HTTP instead.

To have your agent install and set up carrel itself, point it at [install-ai.md](https://raw.githubusercontent.com/arunoruto/carrel/main/install-ai.md).

Clients that don't pass your shell's environment on need settings in carrel's [config file](/docs/reference/configuration.md) rather than in environment variables.

## How an assistant reads a paper

The server tells the assistant to `search`, then `get_paper` (metadata, abstract, approved summary and keywords, figure list), and only then `read_paper`, which returns the full text in chunks so long papers don't flood its context. Figures come as images (`get_figure`), supplementary text through `read_paper`'s `file` option. The [MCP tools](/docs/reference/mcp-tools.md) page lists everything.

## Drafts: the AI proposes, you decide

Everything an assistant writes (a summary, keywords, notes) becomes a draft in the paper's `drafts/` folder; nothing changes in your library until you approve it:

```sh
carrel drafts
carrel approve ho2020denoising summary             # replaces the summary
carrel approve ho2020denoising keywords            # adds to the .bib's keywords
carrel approve ho2020denoising notes               # appended to your notes
carrel approve ho2020denoising references          # adds references read from the full text
carrel authors approve 1                           # a proposal that two spellings are one person
carrel reject ho2020denoising summary
```

In the TUI, `d` shows each draft next to the current version: `a` approves, `r` rejects, `e` edits it first.

## Things to ask

- *"Read the papers in my thesis project that have no summary yet and propose summaries and keywords."*
- *"Which papers cited by ho2020denoising am I missing?"*
- *"What should I read next for my thesis project?"* (`suggest`, see [the citation graph](/docs/guide/citations.md))
- *"hiesinger2011ages has no known references; read them from its full text."*
- *"Find what my library says about classifier-free guidance, with page references."*
- *"Write the related-work paragraph for these five papers and give me their BibTeX."*
