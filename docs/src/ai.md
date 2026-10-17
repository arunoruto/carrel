# Working with an AI

`carrel mcp` serves the library over the [Model Context Protocol](https://modelcontextprotocol.io), so assistants such as Claude can search it, read papers and figures, follow citations and propose summaries, keywords and notes.

## Connecting

Claude Code:

```sh
claude mcp add --scope user carrel -- carrel mcp
```

Claude Desktop, and other clients configured with JSON:

```json
{ "mcpServers": { "carrel": { "command": "carrel", "args": ["mcp"] } } }
```

Clients that don't pass your shell's environment on need settings in carrel's [config file](configuration.md) rather than in environment variables.

## How an assistant reads a paper

The server tells the assistant to `search`, then `get_paper` (metadata, abstract, approved summary and keywords, figure list), and only then `read_paper`, which returns the full text in chunks so long papers don't flood its context. Figures come as images (`get_figure`), supplementary text through `read_paper`'s `file` option. The [MCP tools](mcp-tools.md) page lists everything.

## Drafts: the AI proposes, you decide

Everything an assistant writes (a summary, keywords, notes) becomes a draft in the paper's `drafts/` folder; nothing changes in your library until you approve it:

```sh
carrel drafts
carrel approve ho2020denoising summary             # replaces the summary
carrel approve ho2020denoising keywords            # replaces the .bib's keywords
carrel approve ho2020denoising notes               # appended to your notes
carrel reject ho2020denoising summary
```

In the TUI, `d` shows each draft next to the current version: `a` approves, `r` rejects, `e` edits it first.

## Things to ask

- *"Read the papers in my thesis project that have no summary yet and propose summaries and keywords."*
- *"Which papers cited by ho2020denoising am I missing?"*
- *"Find what my library says about classifier-free guidance, with page references."*
- *"Write the related-work paragraph for these five papers and give me their BibTeX."*
