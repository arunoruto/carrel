# The library

The library is a folder of plain files, by default `~/Documents/carrel` (see [Configuration](configuration.md)). carrel reads them fresh every time, so you can edit, sync or version them with any tool ([Sharing a library](sharing.md)).

```text
~/Documents/carrel/
  vaswani2017attention/
    vaswani2017attention.bib   metadata and approved keywords; the key is the folder name
    vaswani2017attention.pdf
    fulltext.md                the extracted text
    figures/                   the extracted figures
    summary.md                 an approved summary
    notes.md                   your notes, and approved AI notes
    references.json            what it cites, each with its source (see The citation graph)
    drafts/                    AI proposals waiting for review
    supplementary/             extra files; PDFs among them get their text next to them
  projects/
    thesis.txt                 a project: one paper id per line
  people/
    clark-roger-n.json         a confirmed person: the spellings that are them (see The citation graph)
```

## Citation keys

A paper's key names its folder, its `.bib` and PDF, and its BibTeX entry, so `\cite{vaswani2017attention}` and Typst's `@vaswani2017attention` work directly.

By default a key is the first author's family name, the year and the first meaningful title word, in ASCII: `vaswani2017attention`, `muller2021uber`. A second paper with the same key gets `b`, `c`, … appended. The format is a template you can change ([Configuration](configuration.md#citation-keys)).

Keys never change on their own, so citations keep working. `carrel rename <id> [new]` changes one on purpose: folder, files, `.bib` key and project lists together.

## The .bib files

Each paper's metadata is a single BibTeX entry. carrel keeps values exactly as written, so hand edits and fields it doesn't know survive. Text from the web is escaped for LaTeX, while math such as `$\beta$` passes through untouched. Keywords, your tags and those an AI proposed and you approved, live in the entry's `keywords` field; approving more adds to them.

`carrel edit <id>` opens the `.bib` in `$VISUAL`/`$EDITOR` and checks it afterwards.

## Search

`carrel search` (and `/` in the TUI) ranks papers by where words appear: titles count most, then keywords, authors, abstracts and summaries, notes, and the full text including supplementary material. Words match while you type (`diffu` finds diffusion) and by stem (`network` finds networks). If no paper has every word, any word counts.

Search a single field with `field:word`: `title`, `author`, `keyword`, `year`, `venue`, `doi`, `abstract`, `summary`, `notes`, `text`. Search a project with `project:name`.

The index lives in SQLite in `~/.cache/carrel` and only mirrors the files; carrel updates it before each search and rebuilds it if it is ever damaged.

## From scripts

`carrel list --json` (or `-p project` for one project, `show <id> --json` for one paper) prints everything carrel knows about each paper, the same as the MCP tool `get_paper`: metadata, abstract, summary, keywords, notes, figures, projects and the paths of its files. A script can then read the extracted text directly, for example to build its own search index:

```python
import json, subprocess

out = subprocess.run(["carrel", "list", "--json", "-p", "moon"], capture_output=True, text=True, check=True)
for paper in json.loads(out.stdout):
    if paper["files"]["fulltext"]:
        text = open(paper["files"]["fulltext"], encoding="utf-8").read()
        print(paper["id"], paper["doi"], len(text))
```

To share a library with others, see [Sharing a library](sharing.md).
