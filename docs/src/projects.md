# Projects and bibliographies

A project groups papers, such as everything a thesis chapter or a talk cites. It is a text file, `projects/<name>.txt`, with one paper id per line; `#` starts a comment.

```sh
carrel project add thesis vaswani2017attention ho2020denoising
carrel project                                     # list projects
carrel project show thesis
carrel project remove thesis ho2020denoising       # the paper stays in the library
carrel project delete thesis                       # the papers stay in the library
```

In the TUI, `P` chooses which project to show and `+`/`-` put the selected paper into a project or take it out.

## Bibliographies

A project's bibliography is its papers' `.bib` files joined:

```sh
carrel bib -p thesis > ~/thesis/refs.bib
carrel bib vaswani2017attention ho2020denoising    # just these
carrel bib                                         # the whole library
```

## Keeping a project in step with your writing

```sh
carrel project sync thesis ~/thesis/*.tex ~/talk/slides.typ
```

reads the citations of LaTeX (`\cite`, `\citep`, `\parencite`, `\nocite`, …) and Typst (`@key`, `#cite(<key>)`) files, ignoring commented-out ones, and

- puts every cited paper that's in the library into the project,
- lists cited keys that aren't in the library (in Typst this includes `@fig:…` labels, which look the same),
- lists the project's papers that nothing cites.
