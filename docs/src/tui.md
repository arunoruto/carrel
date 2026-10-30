# The TUI

The [web UI](web.md) covers the same ground in the browser. `carrel` without a command opens the terminal UI (unless your [config](configuration.md#what-carrel-alone-does) says otherwise); `carrel tui` always does. It has four tabs, each with up to three panes that narrow from left to right, and a *Selected* bar below them. The focused pane has the accent border; `h`/`l` move the focus.

- **Library**: shelves (all papers, your projects, and keywords on at least two papers) › the papers on that shelf, each with its status › the selected paper: authors, venue, keywords, summary, abstract, notes.
- **Graph**: for the paper selected in the Library, the library papers it cites and theirs in turn, the ones citing it, and the papers the library (or the project shown) cites most but doesn't have.
- **People**: everyone who wrote a library paper (✓ confirmed, ? someone might be the same person) › their papers › their spellings, co-authors, and who might be them too.
- **Drafts**: what an AI proposed, for a paper or about people, next to what it would change.

A paper's status:

| Status | Meaning |
|---|---|
| `ready` | its text is extracted: an AI can read it |
| `no text` | it has a PDF whose text couldn't be extracted (`carrel extract <id>`) |
| `no pdf` | metadata only (`p` attaches a PDF) |
| `draft` | an AI proposal waits in the Drafts tab |

## Keys

Everywhere: `Tab`/`[`/`]` or `1`–`4` switch tabs, `h`/`j`/`k`/`l` or the arrows move, `Ctrl-D`/`Ctrl-U` move by ten, `g`/`G` jump to the top or bottom, `J`/`K` scroll the text on the right, `S` [syncs](sharing.md) with the remote in the background, `q` quits. The line at the bottom always shows the keys that work right now.

| Tab | Key | Action |
|---|---|---|
| Library | `/` | filter the papers (full-text search, live); `Esc` clears it |
| | `a` | add: a PDF path, DOI, arXiv id, or a title to look up online |
| | `A` | add a PDF with the file picker |
| | `i` | everything about the paper: ids, files, projects |
| | `o` / `e` / `n` | open its PDF / edit its `.bib` / edit its notes |
| | `p` / `s` | attach its PDF / add a supplementary file |
| | `u` | fetch its metadata again |
| | `r` / `R` | its references / the papers citing it (`Enter` adds one) |
| | `+` / `-` | put it in a project / take it out of the project shown |
| | `D` | delete it, after a `[y/N]` |
| Graph | `Enter` | follow a paper: the graph moves to it |
| | `a` or `Enter` | on a missing paper: add it |
| | `i` | details |
| People | `/` | filter people by name |
| | `Enter` | open the selected paper in the Library |
| | `m` / `x` | confirm the person and the selected *maybe* are one person / different people |
| Drafts | `a` / `r` / `e` | approve / reject / edit the draft |

Adding, downloading and looking things up run in the background; the *Selected* bar shows a spinner meanwhile. The file picker starts in your Downloads folder with the newest PDFs first: `Enter` opens a folder or picks a file, `h` goes up, `~` goes home.

## Themes

The colours come from a theme: Catppuccin Mocha by default, Catppuccin Latte built in for light terminals. Pick one in the config:

```toml
theme = "catppuccin-latte"
```

Your own theme is a TOML file in `~/.config/carrel/themes/<name>.toml`, and `theme = "<name>"` picks it. Copy a built-in one ([Mocha](https://github.com/arunoruto/carrel/blob/main/src/themes/catppuccin-mocha.toml)) to start from; the format is the same as [Tapedeck](https://github.com/arunoruto/tapedeck)'s, so its themes work unchanged. `stripes` colour the book spines in the logo.
