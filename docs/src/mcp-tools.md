# MCP tools

What `carrel mcp` offers an assistant. Paper ids are citation keys. The assistant can read everything; the only thing it can write is drafts.

| Tool | Arguments | Returns |
|---|---|---|
| `search` | `query`, `limit`, `semantic` | best matches first: id, title, authors, year, keywords, where the words matched. `field:word` and `project:name` work. With `semantic`, by [meaning](semantic-search.md): the passage that matched and where it is |
| `list_papers` | `project` | all papers, or a project's |
| `list_projects` | | each project with its paper ids |
| `get_paper` | `id` | metadata, abstract, summary, keywords, notes, figure and supplementary lists, pending drafts, file paths |
| `read_paper` | `id`, `offset`, `length`, `file` | the full text (or a supplementary text file) in chunks, headed by where to continue |
| `get_figure` | `id`, `name` | a figure as an image |
| `related` | `id`, `cited_by` | the papers it cites (each with its source), or that cite it, marked when in the library, with the id to add each by |
| `neighbors` | `id` | the library papers it cites, that cite it and that share authors with it, and its references missing from the library |
| `suggest` | `project`, `limit` | papers the library (or a project) cites most but doesn't have, with who cites them |
| `tree` | `id`, `cited_by`, `depth` | the citation tree among library papers, with titles |
| `authors` | `name` | the people behind the papers: their spellings, library papers, closest co-authors, and who might be the same person |
| `propose` | `id`, `field`, `content` | stores a summary, keywords, notes or references (read from the full text) as a draft for you to review |
| `propose_people` | `spellings`, `same`, `reason` | proposes that spellings are one person or different people, as a draft for you to review |
| `bibtex` | `ids` or `project` | BibTeX entries |
| `doctor` | | the library's problems, each with the command that fixes it |

Paper ids, figure names and file names are checked against the library, so an assistant can't read files outside it.
