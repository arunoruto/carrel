---
title: carrel
layout: hextra-home
---

<div class="hx:mt-6 hx:mb-6">
<img src="images/logo.svg" alt="" width="88" height="88" class="carrel-logo hx:dark:hidden">
<img src="images/logo-dark.svg" alt="" width="88" height="88" class="carrel-logo hx:hidden hx:dark:block">
</div>

<div class="hx:mb-6">
{{< hextra/hero-headline >}}
  A paper library&nbsp;<br class="hx:sm:block hx:hidden" />for you and your AI
{{< /hextra/hero-headline >}}
</div>

<div class="hx:mb-10">
{{< hextra/hero-subtitle >}}
  Add papers by PDF, DOI, arXiv id, ISBN or title.&nbsp;<br class="hx:sm:block hx:hidden" />Plain files you own, searchable from a TUI, the browser and your AI.
{{< /hextra/hero-subtitle >}}
</div>

<div style="display: flex; flex-wrap: wrap; gap: 0.75rem; margin-bottom: 3rem;">
{{< hextra/hero-button text="Get started" link="docs/installation" >}}
{{< hextra/hero-button text="GitHub" link="https://github.com/arunoruto/carrel" style="background: transparent; color: inherit; border: 1px solid currentColor;" >}}
</div>

```sh
curl -fsSL https://raw.githubusercontent.com/arunoruto/carrel/main/install.sh | sh
carrel add 1706.03762      # metadata, PDF, text and figures
carrel integrate claude    # or opencode, codex, gemini, cursor
```

<div class="hx:mt-12"></div>

{{< hextra/feature-grid >}}
  {{< hextra/feature-card
    title="Add anything"
    icon="document-add"
    link="docs/guide/adding-papers"
    subtitle="A PDF, DOI, arXiv id, ISBN, ADS bibcode or title. Metadata from doi.org, Crossref, Semantic Scholar, arXiv, ADS and Open Library; or import a .bib from Zotero, JabRef or I, Librarian."
    style="background: radial-gradient(ellipse at 50% 80%,rgba(76,126,240,0.15),hsla(0,0%,100%,0));"
  >}}
  {{< hextra/feature-card
    title="Plain files you own"
    icon="folder-open"
    link="docs/guide/library"
    subtitle="Each paper is a folder named by its citation key: its .bib, its PDF, the extracted text and figures, a summary and your notes. Edit it by hand, put it in git."
    style="background: radial-gradient(ellipse at 50% 80%,rgba(242,163,58,0.15),hsla(0,0%,100%,0));"
  >}}
  {{< hextra/feature-card
    title="Your AI reads along"
    icon="sparkles"
    link="docs/guide/ai"
    subtitle="An MCP server for Claude Code, opencode, Codex, Gemini and Cursor. Summaries, keywords and notes an AI writes wait as drafts until you approve them."
    style="background: radial-gradient(ellipse at 50% 80%,rgba(232,88,110,0.15),hsla(0,0%,100%,0));"
  >}}
  {{< hextra/feature-card
    title="Bibliographies that keep up"
    icon="academic-cap"
    link="docs/guide/projects"
    subtitle="Projects follow the citations in your LaTeX or Typst files, and give you their .bib in one command."
  >}}
  {{< hextra/feature-card
    title="Citations and people"
    icon="share"
    link="docs/guide/citations"
    subtitle="References and citing papers, the authors behind them and where they worked, as a tree and a map."
  >}}
  {{< hextra/feature-card
    title="Everywhere you work"
    icon="server"
    link="docs/guide/architecture"
    subtitle="A TUI, a web UI with a PDF reader, a CLI, and a server or S3 bucket to share the library between machines."
  >}}
{{< /hextra/feature-grid >}}
