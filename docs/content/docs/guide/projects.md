---
title: "Projects and bibliographies"
weight: 4
aliases: ["/projects.html"]
---

A project groups papers, such as everything a thesis chapter or a talk cites. It is a text file, `projects/<name>.txt`, with one paper id per line; `#` starts a comment.

```sh
carrel project add thesis vaswani2017attention ho2020denoising
carrel project                                     # list projects
carrel project show thesis
carrel project remove thesis ho2020denoising       # the paper stays in the library
carrel project delete thesis                       # the papers stay in the library
carrel project rename thesis dissertation
```

In the TUI, the shelves on the left list the projects. There `+` makes a new one, `r` renames the selected one, `x` makes it private or shared, and `D` deletes it; on a paper, `+`/`-` put it into a project or take it out. In the [web UI](/docs/guide/web.md), the *+* beside *Projects* in the sidebar makes one, and each project's buttons rename, share or delete it.

## Shared and private projects

Projects are shared: with a [group's library](/docs/guide/sharing.md), everyone sees them, and through a [carrel server](/docs/guide/server.md) changes to them show in its activity like any other. A **private** project is yours alone: it lives in `private/projects/` instead, and a carrel server keeps it for your login, on all your machines, without showing it to anyone or in the activity. It needs a token (or a login, in the web UI); an S3 bucket or a shared folder never gets private projects, so without a server they stay on the one machine.

```sh
carrel project add reviewing smith2024 --private   # a new private project
carrel project private thesis                      # make one private
carrel project share thesis                        # share it again
```

`carrel project` marks the private ones. Both kinds share one set of names: a private `thesis` and a shared `thesis` can't both exist. Through a server, deleting a shared project is [asked for](/docs/guide/server.md#activity-changes-reverts-and-deletions), as deleting a paper is; deleting a private one isn't.

## Bibliographies

A project's bibliography is its papers' `.bib` files joined:

```sh
carrel bib -p thesis > ~/thesis/refs.bib
carrel bib vaswani2017attention ho2020denoising    # just these
carrel bib                                         # the whole library
```

### JabRef

[JabRef](https://www.jabref.org) (or any tool that reads BibTeX) can browse the library through one generated file. `--jabref` adds what JabRef needs to open the PDFs and show your projects:

```sh
carrel bib --jabref > ~/Documents/carrel.bib
carrel bib --jabref -p thesis > thesis-jabref.bib   # a project's papers, still with every project as a group
```

- Each entry's `file` field links its PDF and supplementary files, relative to the library, which the file names as its `fileDirectory` (`@Comment{jabref-meta: fileDirectory:…}`).
- Its projects go into `groups`, and every project is a static group in JabRef's group tree.
- `databaseType:biblatex` is set only when the entries use biblatex fields (`journaltitle`, `date`, …); carrel itself writes BibTeX.

Without `--jabref` (`--files` is its old name), `carrel bib` prints the `.bib` files as they are, so a thesis bibliography stays minimal; biber and BibTeX would ignore the extra fields anyway. carrel's own files stay the source: regenerate the file after changes, and edit metadata with `carrel edit`, since JabRef's edits stay in the generated file. Don't commit it to a [shared library](/docs/guide/sharing.md): the library's path in it is this machine's.

Going the other way, `carrel import` reads a JabRef library: see [importing a .bib](/docs/guide/adding-papers.md#importing-a-bib). Exporting with `--jabref` and importing that file into an empty library gives back the papers, their files and the projects.

## Keeping a project in step with your writing

```sh
carrel project sync thesis ~/thesis/*.tex ~/talk/slides.typ
```

reads the citations of LaTeX (`\cite`, `\citep`, `\parencite`, `\nocite`, …) and Typst (`@key`, `#cite(<key>)`) files, ignoring commented-out ones, and

- puts every cited paper that's in the library into the project,
- lists cited keys that aren't in the library (in Typst this includes `@fig:…` labels, which look the same),
- lists the project's papers that nothing cites.
