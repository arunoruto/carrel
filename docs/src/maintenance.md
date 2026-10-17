# Keeping the library healthy

## carrel doctor

```sh
carrel doctor            # problems, each with the command that fixes it
carrel doctor --all      # also each paper without a PDF and each arXiv preprint
carrel doctor --json     # for scripts (and the MCP tool `doctor`)
```

| Check | Level | Meaning |
|---|---|---|
| `broken` | error | a `.bib` that is missing, invalid, or whose key differs from its folder |
| `fields` | warning | missing title, author, year, or the journal/booktitle its type needs |
| `markup` | warning | HTML tags left in a field |
| `macro` | warning | an `@string` macro, undefined outside its original `.bib` |
| `duplicate` | warning | the same DOI, arXiv id or title as another paper |
| `text` | warning | a PDF whose text couldn't be extracted, so an AI can't read it |
| `project` | warning | a project listing a paper that isn't in the library |
| `pdf` | info | no PDF |
| `preprint` | info | an arXiv preprint that may have been published since |

## carrel update

```sh
carrel update girshick2013rich          # fetch the metadata again
carrel update --all
carrel update smith2020 10.1145/123456  # say which paper it is (DOI, arXiv id or title)
```

Fetched fields replace those whose text changed; your keywords and own fields stay, and so does the key. An arXiv preprint switches to its published version when one is known. Every change is printed, and if the key template would now give a different key, carrel says so without renaming.
