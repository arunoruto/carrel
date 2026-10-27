# The citation graph

carrel keeps each paper's reference list, so it knows which of your papers cite each other, which share authors, and which papers your library keeps citing but doesn't have. The cited papers don't need to be in the library.

```sh
carrel refs mccord2011sources          # what it cites, marking what you have
carrel graph clark2009detection        # how it connects to the rest of the library
carrel graph                           # what the library cites most but lacks
carrel graph -p thesis -n 10           # the same for one project
carrel graph --dot | dot -Tsvg > graph.svg
carrel tree mccord2011sources          # what it cites, what those cite, … (-d for depth)
carrel tree pieters2009character --cited-by
```

```text
$ carrel tree mccord2011sources
mccord2011sources  Sources and physical processes responsible for OH/H2O in the lunar soil …
  clark2009detection  Detection of Adsorbed Water and Hydroxyl on the Moon
    pieters2009character  Character and Spatial Distribution of OH/H2O on the Surface of the Moon …
      clark2009detection  (see above)
      sunshine2009temporal  Temporal and Spatial Variability of Lunar Hydration …
    sunshine2009temporal  (see above)
  pieters2009character  (see above)
  sunshine2009temporal  (see above)
```

A paper met again is marked *(see above)* and not followed, so papers citing each other don't loop.

```text
$ carrel graph clark2009detection
cites:     pieters2009character, sunshine2009temporal
cited by:  mccord2011sources, pieters2009character, sunshine2009temporal
authors:   mccord2011sources (Clark, Roger N.)
18 of its references are not in the library (`carrel refs clark2009detection`)

$ carrel graph
  3× Evidence for water ice near the lunar poles (Feldman et al., 2001, Journal of Geophysical Research Atmospheres)  10.1029/2000je001444
      cited by clark2009detection, mccord2011sources, pieters2009character
```

An assistant gets the same through the MCP tools `neighbors`, `tree` and `suggest`, so it can work out what else to read.

## Where references come from

`carrel add` looks up a paper's references when it has a DOI or arXiv id. For papers added another way, `carrel refs <id>` looks them up the first time, and `carrel refs --all` does it for every paper without references. `--refresh` looks them up again.

carrel asks every source in this order, and each adds the references the earlier ones lack. Where two list the same paper, the earlier entry stays, as its details are usually better:

| Source | Covers | Needs |
|---|---|---|
| `ads` ([NASA ADS](https://ui.adsabs.harvard.edu)) | astronomy and planetary science, including conference abstracts without a DOI | a free token in `ADS_API_TOKEN`; skipped without one |
| `openalex` ([OpenAlex](https://openalex.org)) | most journals across all fields, each reference matched to a record | nothing; `OPENALEX_API_KEY` raises the shared daily budget |
| `crossref` ([Crossref](https://www.crossref.org)) | the reference lists publishers deposit, including what OpenAlex can't match: conference abstracts, theses, books. Some entries are only the citation as printed | a DOI |
| `semantic-scholar` | computer science and preprints; asked only when the others find nothing, since some publishers (Wiley, for example) hide their references from it | nothing; `S2_API_KEY` against rate limits |
| `ai` | anything with a reference list in its full text | an assistant, and your approval |

Every reference records its source, so you can tell what came from a database and what an AI read.

## When no source knows a paper

Old book chapters, theses and conference abstracts are often missing from all sources. An assistant can read the reference list in the paper's full text and propose it, as a draft like everything else it writes:

```sh
carrel drafts
carrel approve hiesinger2011ages references   # adds the references not known yet
```

Once approved, those references are marked `ai`. Looking a paper up again later keeps them, unless a source now has the same papers.

## How papers are matched

A reference matches a library paper by DOI, arXiv id or title (ignoring case and punctuation), so renaming a paper keeps its links.

## Who is who

Names are written many ways: *Pieters, Carle M.*, *C. M. Pieters*, *Pieters, C.*. carrel counts two spellings as one person when their initials fit and there is evidence: a co-author in common, or the same first name. Without evidence they stay apart, so two different *Zhang, Y.* don't merge, and *James W. Head* never merges with a *John Head*. The author lists of the references count too, which gives far more co-authors than the library alone.

```text
$ carrel authors pieters
Pieters, Carle M.  (also Pieters, C. M.; Carle M. Pieters)
  papers:     mccord2011sources, pieters2009character
  co-authors: Taylor, L. A. (6), McCord, T. B. (5), Boardman, Joseph W. (4), …

$ carrel authors clark
Clark, Roger N.
  papers:     clark2009detection
  co-authors: none
  maybe also: Clark, R. N. (cited only); Clark, R. N. (mccord2011sources, pieters2009character)  [no co-author or first name in common]
```

*maybe also* lists names that fit but lack evidence, with their library papers or *cited only* for someone who appears only in references. Here Roger N. Clark wrote his paper alone and every reference spells him *R. N. Clark*, so carrel can't tell; you, or an assistant reading the papers, can. An assistant gets the same through the MCP tool `authors`.

### Confirming people

When you know better, say so; it comes before any evidence:

```sh
carrel authors same "Clark, Roger N." "Clark, R. N."        # one person
carrel authors different "Head, J." "Head, James W."        # two people
```

Each confirmed person is a file, `people/<id>.json`, listing the spellings that belong to them and nobody else, so the decision travels with the library when you [share it](sharing.md). Confirming again with a spelling that's already taken joins the two.

An assistant can propose such decisions too (the MCP tool `propose_people`, with its reasons). They wait as drafts:

```sh
carrel drafts                 # people 1   one person: Clark, Roger N.; R. N. Clark  (both at USGS Denver, same topic)
carrel authors approve 1
carrel authors reject 1
```

The references live in `<key>/references.json`, a plain JSON list, so they travel with the library when you [share it](sharing.md).
