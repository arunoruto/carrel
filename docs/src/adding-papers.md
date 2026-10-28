# Adding papers

`carrel add` takes any mix of:

| Source | Example | What happens |
|---|---|---|
| arXiv id or URL | `2006.11239`, `arXiv:hep-th/9711200`, `https://arxiv.org/abs/2006.11239v2` | metadata from arXiv's DOI, the PDF downloaded |
| DOI or URL | `10.1038/nature14539`, `https://doi.org/…`, publisher URLs containing the DOI | metadata from doi.org (Crossref, DataCite, …) |
| PDF | `paper.pdf`, `'~/Downloads/a paper.pdf'` | text extracted; its DOI or arXiv id read from the first page, or else its title looked up |
| exact title | `"Attention Is All You Need"` | looked up on Crossref, Semantic Scholar and arXiv |

A title that doesn't match exactly prints the closest papers with the id to add each by. In the TUI, `a` with a title shows the results to pick from.

If an arXiv version exists, its PDF is downloaded, while the metadata comes from the published DOI when there is one.

## From your Downloads

Papers pile up in Downloads first. `carrel consume` lists the PDFs there from the last 30 days and asks about each; the ones you say yes to move into the library (out of Downloads), the ones you say no to aren't asked about again (`--again` asks anyway):

```text
$ carrel consume
2 PDFs from the last 30 days. Added ones move out of their folder.
Enter adds those naming a DOI or arXiv id [Y/n] and skips the rest [y/N]; q stops.

/home/me/Downloads/2301.01234v2.pdf
  arXiv id 2301.01234 in its name: add? [Y/n/q]
  added smith2023learning
/home/me/Downloads/eInvoice_1922.pdf
  no DOI or arXiv id in its name or metadata: add? [y/N/q]
```

The DOI or arXiv id only sets the default answer: a DOI in the file may also be one it cites, and plenty of papers keep theirs where carrel doesn't look before extracting. Adding decides from the text.

Other folders, and how far back to look:

```toml
[consume]
folders = ["~/Downloads", "~/Nextcloud/papers"]
days = 30
auto = false
```

Without a terminal, from a timer, `carrel consume` doesn't ask. It tells you through the [notifiers](configuration.md#notifications) about the PDFs new since last time, once each. With `auto = true` (or `carrel consume --yes`) it adds the ones it can identify as papers, by a DOI, an arXiv id or a known title, and tells you what it added and what it left. Invoices and letters stay where they are. A systemd user timer every 10 minutes:

```ini
# ~/.config/systemd/user/carrel-consume.service
[Service]
ExecStart=%h/.nix-profile/bin/carrel consume

# ~/.config/systemd/user/carrel-consume.timer
[Timer]
OnCalendar=*:0/10
[Install]
WantedBy=timers.target
```

`systemctl --user enable --now carrel-consume.timer` starts it.

## Duplicates

A paper is the same as one in the library when its DOI, its arXiv id or its title (ignoring case, punctuation and accents) matches. Adding it again is refused, except to give a paper without a PDF its PDF.

## PDFs later

Papers added by DOI or title can get their PDF afterwards:

```sh
carrel attach lecun2015deep ~/Downloads/lecun.pdf
carrel attach kingma2014adam                       # no file: downloaded from arXiv
```

In the TUI, `p` opens a file picker, newest PDFs first.

## Supplementary material

```sh
carrel addto ho2020denoising appendix.pdf https://example.org/code.zip
```

Files land in the paper's `supplementary/` folder. PDFs among them get their text extracted next to them (`appendix.pdf` → `appendix.md`), which search and your AI can read too. In the TUI: `s`.

## Importing a .bib

```sh
carrel import ~/thesis/refs.bib -p thesis --pdfs
```

Every entry becomes a paper with its key unchanged, so your documents' citations keep working. Entries already in the library are matched instead, `@string` macros are expanded, keys that can't name a folder get a new one (reported), and with `--pdfs` entries with an arXiv id get their PDF.

## Moving from I, Librarian

[I, Librarian](https://github.com/mkucej/i-librarian-free) keeps its library in a data folder: an SQLite database (`database/main.db`) and the PDFs (`pdfs/`). carrel reads it directly, which keeps more than a BibTeX export would:

```sh
carrel import-ilibrarian ~/backup/i-librarian/data   # copy the folder out first; carrel only reads it
carrel extract --all                                 # then the text and figures
carrel refs --all                                    # and the references
```

- Every item becomes a paper with a carrel key. Its old key (`Egel-2017-ID1`) stays in the `ids` field, which biber accepts as an alias, so documents citing it still compile.
- Tags join the publisher's keywords in `keywords`, the field JabRef and Zotero use for tags too.
- Notes, highlights (joined back into passages, with their page) and margin notes go into `notes.md`.
- Projects become projects; spaces in their names become `-`.
- Items I, Librarian holds twice become one paper. Importing again adds nothing new.
- HTML in titles and abstracts becomes LaTeX: `M<sup>3</sup>` turns into `M$^{3}$`.

With I, Librarian in Docker, the data folder is the volume mounted at `/app/data`; `docker cp <container>:/app/data ./ilibrarian-data` copies it out.

## When there is no identifier

A PDF without DOI or arXiv id whose title isn't found still gets added, with its title from the PDF. Tell carrel which paper it is later:

```sh
carrel update attention 10.48550/arXiv.1706.03762   # or an arXiv id, or the exact title
carrel rename attention                              # then a key from the new metadata
```
