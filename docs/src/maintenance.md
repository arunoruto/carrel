# Keeping the library healthy

## carrel doctor

```sh
carrel doctor            # problems, each with the command that fixes it
carrel doctor --all      # also each paper without a PDF and each arXiv preprint
carrel doctor --online   # also looks up every DOI, for those that belong to another work (a minute or two)
carrel doctor --json     # for scripts (and the MCP tool `doctor`)
```

| Check | Level | Meaning |
|---|---|---|
| `broken` | error | a `.bib` that is missing, invalid, or whose key differs from its folder |
| `fields` | warning | missing title, author, year, or the journal/booktitle its type needs |
| `markup` | warning | HTML tags left in a field |
| `macro` | warning | an `@string` macro, undefined outside its original `.bib` |
| `duplicate` | warning | the same DOI, arXiv id, ISBN or title as another paper |
| `doi` | warning | with `--online`: its DOI belongs to another work, or to none |
| `text` | warning | a PDF whose text couldn't be extracted, so an AI can't read it |
| `project` | warning | a project listing a paper that isn't in the library |
| `pdf` | info | no PDF |
| `preprint` | info | an arXiv preprint that may have been published since |

## carrel update

```sh
carrel update girshick2013rich          # fetch the metadata again
carrel update --all
carrel update smith2020 10.1145/123456  # say which paper it is (DOI, arXiv id, ISBN, ADS bibcode or title)
```

Fetched fields replace those whose text changed; your keywords and own fields stay, and so does the key. An arXiv preprint switches to its published version when one is known. Every change is printed, and if the key template would now give a different key, carrel says so without renaming.

A DOI whose work has another title is refused rather than believed: imports can bring another work's DOI along, such as a review of the book. Fix the `doi` field, or pass the DOI on the command line if it is right after all.

**Books** (`@book`) only get the fields they lack, as publishers' records and Open Library are less careful than a `.bib`: from their DOI if they have one, and from Open Library by their ISBN. The ISBN comes from the `.bib`, else the copyright page in the book's text, else Open Library's edition of that title, author and year; another year's edition doesn't count. `carrel update --all` includes books.

**Papers without DOI or arXiv id**, such as conference abstracts and older papers, get what they lack from [NASA ADS](https://ui.adsabs.harvard.edu) when there is an ADS token: by the bibcode in their `adsurl` (as ADS's BibTeX has it), else ADS's record of their title, first author and year (that year's, as one abstract can be given at two conferences). That fills in an LPSC abstract's series and number, or an old paper's journal and volume. If ADS knows a DOI, the next `carrel update` uses it.
