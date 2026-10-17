# The TUI

Run `carrel` without a command. The left pane lists papers, the right one shows the selected paper: metadata, keywords, projects, supplementary files, its summary, abstract and notes.

| Key | Action |
|---|---|
| `j`/`k`, arrows | move |
| `J`/`K`, PgDn/PgUp | scroll the details |
| `/` | search (live); `Esc` clears it |
| `a` | add: a PDF path, DOI, arXiv id, or a title to look up online |
| `i` | import a PDF with the file picker |
| `p` | attach a PDF to the selected paper |
| `s` | add a supplementary file |
| `u` | update the selected paper's metadata |
| `o` | open its PDF |
| `e` | edit its `.bib` |
| `n` | edit its notes |
| `r` / `R` | its references / the papers citing it (`Enter` adds one) |
| `P` | choose a project to show |
| `+` / `-` | put the paper into a project / take it out of the one shown |
| `d` | review drafts: `a` approve, `r` reject, `e` edit |
| `q` | quit |

Adding, downloading and looking things up run in the background; the header shows what's going on. The file picker starts in your Downloads folder with the newest PDFs first: `Enter` opens a folder or picks a file, `Backspace` goes up, `~` goes home.
