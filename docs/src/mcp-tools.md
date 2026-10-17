# MCP tools

What `carrel mcp` offers an assistant. Paper ids are citation keys. The assistant can read everything; the only thing it can write is drafts.

| Tool | Arguments | Returns |
|---|---|---|
| `search` | `query`, `limit` | best matches first: id, title, authors, year, keywords, where the words matched. `field:word` and `project:name` work |
| `list_papers` | `project` | all papers, or a project's |
| `list_projects` | | each project with its paper ids |
| `get_paper` | `id` | metadata, abstract, summary, keywords, notes, figure and supplementary lists, pending drafts, file paths |
| `read_paper` | `id`, `offset`, `length`, `file` | the full text (or a supplementary text file) in chunks, headed by where to continue |
| `get_figure` | `id`, `name` | a figure as an image |
| `related` | `id`, `cited_by` | the papers it cites, or that cite it, marked when in the library, with the id to add each by |
| `propose` | `id`, `field`, `content` | stores a summary, keywords or notes as a draft for you to review |
| `bibtex` | `ids` or `project` | BibTeX entries |
| `doctor` | | the library's problems, each with the command that fixes it |

Paper ids, figure names and file names are checked against the library, so an assistant can't read files outside it.
