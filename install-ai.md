# Installing carrel: a guide for AI agents

A human asked you to install carrel and set it up for them. Follow this guide step by step. The full documentation is at https://arunoruto.github.io/carrel/. When you're unsure about a command, read its page or run `carrel <command> --help`. Don't guess.

## What carrel is

carrel is a paper library for researchers and their AI. Every paper is a folder holding its `.bib`, its PDF, the extracted text and figures, a summary and notes. The default library is `~/Documents/carrel`. A SQLite index makes it searchable. carrel has a CLI, a TUI (`carrel`), a web UI (`carrel web`) and an MCP server (`carrel mcp`), so agents like you can search papers, read them and propose summaries. Everything an AI writes becomes a draft that the human approves.

## Rules for you

- **Ask before writing outside carrel's own files.** That means agent configs, shell profiles and Nix files. Say what you'll change, then wait for a yes.
- **Never ask for API tokens in the chat, and never print them.** Have the human write each token into a file (`~/.config/carrel/<name>-token`, mode 600) and point the config at that file.
- **Ask, don't assume.** Where the library lives, which agents the human uses, and whether a library already exists elsewhere (a server, another machine) are the human's to decide.
- **Commands only from this guide or `--help`.** Don't invent flags or config keys. carrel's config rejects unknown keys.
- **Don't fill the library with test papers.** Add a paper only when the human names it.

## 1. Look at the machine

```sh
uname -sm
command -v carrel && carrel --version
command -v nix; test -e /etc/NIXOS && echo nixos
command -v home-manager
command -v uv docling
```

- **carrel is already installed:** skip to step 3, but check its version against the latest release.
- **Windows:** there is no native build. Use WSL and follow the Linux steps inside it.

## 2. Install carrel

**With Nix, and especially with Home Manager**, use the flake. carrel's package then brings its own text extractor, and the Home Manager module runs the services. Show the human this and ask where it goes in their configuration:

```nix
{
  inputs.carrel.url = "github:arunoruto/carrel";
  # in a Home Manager module:
  imports = [ inputs.carrel.homeManagerModules.default ];
  home.packages = [ inputs.carrel.packages.${pkgs.stdenv.hostPlatform.system}.default ];
  services.carrel = {
    package = inputs.carrel.packages.${pkgs.stdenv.hostPlatform.system}.default;
    web.enable = true;       # http://127.0.0.1:7700
    consume.enable = true;   # picks up PDFs from Downloads
    # docling.package = pkgs.docling;  # better extraction; large (PyTorch)
  };
}
```

With Nix but without Home Manager: `nix profile install github:arunoruto/carrel`.

**Without Nix** (Linux x86_64 or arm64, macOS on Apple silicon), use the install script. It verifies the checksum and installs to `~/.local/bin`:

```sh
curl -fsSL https://raw.githubusercontent.com/arunoruto/carrel/main/install.sh | sh
```

If `~/.local/bin` is not on `PATH`, ask before adding it to the human's shell profile. Other routes, such as Cargo, bin and Docker, are at https://arunoruto.github.io/carrel/docs/installation/.

**Text extraction:** outside Nix and Docker, carrel needs `uv`, which fetches pymupdf4llm on first use, or `docling`. If neither is there, offer to install uv: `curl -LsSf https://astral.sh/uv/install.sh | sh`. Without either, papers are still added, just without their text.

## 3. Configure

`carrel config` prints the config file's path, the library folder, the extractor and the tokens it found. The config lives at `~/.config/carrel/config.toml`. Every setting is optional, so write only what the human wants:

```toml
library = "~/Documents/carrel"    # ask; the default is fine for most people
# docling = "http://host:5001"    # a docling-serve on another machine, if they have one

[tokens]                          # each one optional; files that the human writes themselves
# ads = { file = "~/.config/carrel/ads-token" }   # NASA ADS: astronomy and planetary science
# s2 = { file = "~/.config/carrel/s2-token" }     # Semantic Scholar: fewer rate limits

# [embeddings]                    # semantic search, if they run Ollama with an embedding model
# ollama = "http://localhost:11434"
# model = "bge-m3"
```

Ask about the field: astronomers want an ADS token (https://ui.adsabs.harvard.edu/user/settings/token). Config details: https://arunoruto.github.io/carrel/docs/reference/configuration/.

**A library that already exists** on a server or another machine is shared with `[remote]` and `carrel sync` (https://arunoruto.github.io/carrel/docs/guide/sharing/). Ask the human for the remote's details instead of making up a new one.

## 4. Bring in their papers

Ask what they have now:

| They have | Command |
|---|---|
| a `.bib` (Zotero export, LaTeX project) | `carrel import refs.bib` (`-p thesis` puts the entries in a project; `--pdfs` downloads arXiv PDFs) |
| a JabRef library | `carrel import library.bib`; its file links and groups come along |
| I, Librarian | `carrel import-ilibrarian <its data folder>` |
| a folder of PDFs | `carrel add ~/Papers/*.pdf` (each PDF's DOI or title is looked up) |
| DOIs or arXiv ids | `carrel add 10.1038/nature14539 1706.03762` |

Then run `carrel refs --all` for references and affiliations. If they set up Ollama, also run `carrel embed --all`. Both take a while on a large library.

## 5. Connect the agents

```sh
carrel integrate                  # lists claude, opencode, codex, gemini and cursor, and which have carrel
carrel integrate claude           # register carrel's MCP server with one of them; ask first
```

After that, the agent needs a restart. If `carrel integrate` doesn't exist (a release older than this guide), add the server by hand:

- Claude Code: `claude mcp add --scope user carrel -- carrel mcp`
- opencode (`~/.config/opencode/opencode.json`): `"mcp": { "carrel": { "type": "local", "command": ["carrel", "mcp"], "enabled": true } }`
- Codex (`~/.codex/config.toml`): `[mcp_servers.carrel]`, then `command = "carrel"` and `args = ["mcp"]`
- Gemini CLI (`~/.gemini/settings.json`) and Cursor (`~/.cursor/mcp.json`): `"mcpServers": { "carrel": { "command": "carrel", "args": ["mcp"] } }`

For a library on a carrel server, the agent talks MCP over HTTP to `https://<server>/mcp` with an `Authorization: Bearer <token>` header (https://arunoruto.github.io/carrel/docs/guide/server/#connecting-an-ai).

## 6. Keep it running (optional)

Without Home Manager:

```sh
carrel service install web        # the web UI at every login; `carrel web --get-url` prints the link to open
carrel service install consume    # new PDFs in Downloads: the human is told about them, or they are added with [consume] auto = true
carrel service                    # what runs
```

On Linux these are systemd user units. On macOS they are launchd agents, and macOS asks before carrel may read Downloads: tell the human to allow it. The services take `PATH` and carrel's variables from the shell you run this in.

## 7. Check and hand over

```sh
carrel config
carrel doctor
```

Then show the human the basics: `carrel` (the TUI), `carrel web`, `carrel search <words>`, `carrel add <doi|arXiv id|pdf>`. Remind them that AI proposals wait in `carrel drafts` until they approve them. Suggest a first request to you, such as: *"Read the papers in my library that have no summary yet and propose summaries and keywords."*
