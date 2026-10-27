//! MCP server: lets an AI assistant search and read the library, and propose drafts.

use anyhow::Context;
use base64::Engine;
use carrel::{Field, Library, Paper};
use rmcp::{
    ErrorData, ServerHandler, ServiceExt,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig},
    tool, tool_handler, tool_router,
    transport::stdio,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

const INSTRUCTIONS: &str = "carrel is the user's personal library of research papers. Paper ids are \
BibTeX citation keys. The user groups papers into projects (`list_projects`). Find papers with \
`search` (or `list_papers`), then `get_paper`: read its summary and \
keywords before loading the full text with `read_paper`, which returns it in chunks. View figures with \
`get_figure`. When you have read a paper that has no summary or keywords yet, write them with `propose`; \
they become drafts the user reviews. Use `bibtex` for citations. To find what else to read, `neighbors` \
shows how a paper connects to the library (citations both ways, shared authors), `suggest` lists the \
papers the library cites most but doesn't have, and `authors` who wrote what and with whom. Each reference says where it came from; when no source \
knows a paper's references, read the reference list in its full text and `propose` them as `references`.";

#[derive(Clone)]
struct Server {
    lib: Library,
}

#[derive(Deserialize, JsonSchema)]
struct SearchArgs {
    /// Words to look for in titles, authors, keywords, abstracts, summaries, notes and full text.
    /// `field:word` searches one field (title, author, keyword, year, venue, abstract, summary, notes, text);
    /// `project:name` searches within a project.
    query: String,
    /// Maximum number of results (default 10).
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct IdArgs {
    /// Paper id (its citation key), e.g. "vaswani2017attention".
    id: String,
}

#[derive(Deserialize, JsonSchema)]
struct ReadArgs {
    /// Paper id.
    id: String,
    /// A text file from the paper's supplementary list (e.g. "appendix.md") instead of the paper itself.
    file: Option<String>,
    /// Character offset to start from (default 0).
    offset: Option<usize>,
    /// Maximum number of characters to return (default 20000).
    length: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct FigureArgs {
    /// Paper id.
    id: String,
    /// Figure file name, as listed by get_paper.
    name: String,
}

#[derive(Deserialize, JsonSchema)]
struct ProposeArgs {
    /// Paper id.
    id: String,
    /// What to write: summary (Markdown), keywords (one per line or comma-separated), notes (Markdown, appended
    /// to existing notes once approved) or references (a JSON array of {title, authors (full names), year,
    /// venue, doi, arxiv}, read from the paper's reference list; those not known yet are added once approved).
    field: Field,
    content: String,
}

#[derive(Deserialize, JsonSchema)]
struct AuthorsArgs {
    /// Only people with a spelling containing this, e.g. a family name.
    name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct SuggestArgs {
    /// Only what this project's papers cite.
    project: Option<String>,
    /// Maximum number of papers (default 20).
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct BibArgs {
    /// Paper ids.
    #[serde(default)]
    ids: Vec<String>,
    /// A project, instead of ids. Without either: the whole library.
    project: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct RelatedArgs {
    /// Paper id.
    id: String,
    /// The papers citing it, instead of the papers it cites.
    #[serde(default)]
    cited_by: bool,
}

#[derive(Deserialize, JsonSchema)]
struct ListArgs {
    /// Only the papers in this project.
    project: Option<String>,
}

fn reply(result: anyhow::Result<String>) -> Result<CallToolResult, ErrorData> {
    Ok(match result {
        Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
        Err(e) => CallToolResult::error(vec![ContentBlock::text(format!("{e:#}"))]),
    })
}

fn brief(p: &Paper) -> Value {
    json!({
        "id": p.id,
        "title": p.bib.title(),
        "authors": p.short_authors(),
        "year": p.bib.year(),
        "venue": p.bib.venue(),
        "keywords": p.bib.keywords(),
        "has_summary": p.get(Field::Summary).is_some(),
    })
}

/// Everything about a paper; `get_paper` and `carrel list --json` share it.
pub fn details(lib: &Library, p: &Paper) -> Value {
    let b = &p.bib;
    let fulltext = Some(p.dir.join("fulltext.md")).filter(|f| f.exists());
    json!({
        "id": p.id,
        "title": b.title(),
        "authors": b.authors(),
        "year": b.year(),
        "venue": b.venue(),
        "doi": b.doi(),
        "arxiv": b.arxiv(),
        "abstract": b.get("abstract"),
        "keywords": b.keywords(),
        "summary": p.get(Field::Summary),
        "notes": p.get(Field::Notes),
        "figures": p.figures(),
        "supplementary": p.supplementary(),
        "fulltext_chars": p.fulltext().map(|t| t.chars().count()),
        "pending_drafts": p.drafts().iter().map(|f| f.name()).collect::<Vec<_>>(),
        "projects": lib.projects_of(&p.id),
        "files": { "dir": p.dir, "bib": p.bib_path(), "pdf": p.pdf(), "fulltext": fulltext },
    })
}

fn pretty(value: Value) -> String {
    serde_json::to_string_pretty(&value).unwrap_or_default()
}

#[tool_router]
impl Server {
    #[tool(
        description = "Search the paper library. Returns the best matches first, with id, title, authors, year, keywords and where the words matched."
    )]
    async fn search(&self, Parameters(a): Parameters<SearchArgs>) -> Result<CallToolResult, ErrorData> {
        reply(self.lib.search(&a.query, a.limit.unwrap_or(10)).map(|hits| {
            let hits = hits.iter().map(|(p, snippet)| {
                let mut paper = brief(p);
                paper["match"] = json!(snippet);
                paper
            });
            pretty(hits.collect())
        }))
    }

    #[tool(description = "List the papers in the library, or in one project, sorted by id.")]
    async fn list_papers(&self, Parameters(a): Parameters<ListArgs>) -> Result<CallToolResult, ErrorData> {
        let papers = match a.project {
            Some(name) => self.lib.project_papers(&name).map(|(papers, _)| papers),
            None => Ok(self.lib.papers().0),
        };
        reply(papers.map(|papers| pretty(papers.iter().map(brief).collect())))
    }

    #[tool(description = "The user's projects, each with the ids of its papers.")]
    async fn list_projects(&self) -> Result<CallToolResult, ErrorData> {
        let projects = self.lib.projects().into_iter().map(|name| {
            let ids = self.lib.project(&name).unwrap_or_default();
            json!({ "name": name, "papers": ids })
        });
        reply(Ok(pretty(projects.collect())))
    }

    #[tool(
        description = "One paper's metadata, abstract, approved summary, keywords, notes, figure names, pending drafts and file paths. Read this before the full text."
    )]
    async fn get_paper(&self, Parameters(a): Parameters<IdArgs>) -> Result<CallToolResult, ErrorData> {
        reply(self.lib.get(&a.id).map(|p| pretty(details(&self.lib, &p))))
    }

    #[tool(
        description = "Read a paper's full text (Markdown), or one of its supplementary text files, in chunks. The first line says where the chunk ends and which offset to continue from."
    )]
    async fn read_paper(&self, Parameters(a): Parameters<ReadArgs>) -> Result<CallToolResult, ErrorData> {
        reply(self.lib.get(&a.id).and_then(|p| {
            let text = match &a.file {
                // Only names from the listing, so the AI cannot read arbitrary files.
                Some(name) if p.supplementary().contains(name) => {
                    std::fs::read_to_string(p.dir.join(carrel::SUPPLEMENTARY).join(name)).context("not a text file")?
                }
                Some(name) => anyhow::bail!("no supplementary file {name:?}; get_paper lists them"),
                None => p.fulltext().context("this paper has no extracted text (no PDF, or extraction failed)")?,
            };
            Ok(carrel::chunk(&text, a.offset.unwrap_or(0), a.length.unwrap_or(0)))
        }))
    }

    #[tool(description = "View one of a paper's figures as an image.")]
    async fn get_figure(&self, Parameters(a): Parameters<FigureArgs>) -> Result<CallToolResult, ErrorData> {
        let image = self.lib.get(&a.id).and_then(|p| {
            // Only names from the listing, so the AI cannot read arbitrary files.
            anyhow::ensure!(p.figures().contains(&a.name), "no figure {:?}; get_paper lists them", a.name);
            Ok(std::fs::read(p.dir.join("figures").join(&a.name))?)
        });
        match image {
            Ok(bytes) => {
                let mime = if a.name.ends_with(".png") { "image/png" } else { "image/jpeg" };
                let data = base64::engine::general_purpose::STANDARD.encode(bytes);
                Ok(CallToolResult::success(vec![ContentBlock::image(data, mime)]))
            }
            Err(e) => reply(Err(e)),
        }
    }

    #[tool(
        description = "The papers a paper cites, each with where that reference came from (openalex, semantic-scholar, ads, or ai: read from the full text and approved by the user), or (cited_by) the papers citing it, from Semantic Scholar. Each says whether it is in the library; suggest missing ones to the user with their `add_with` value (`carrel add <add_with>`)."
    )]
    async fn related(&self, Parameters(a): Parameters<RelatedArgs>) -> Result<CallToolResult, ErrorData> {
        if !a.cited_by {
            return reply(self.lib.references(&a.id, false).map(|found| {
                let refs = found.iter().map(|(r, in_library)| {
                    let mut r = json!(r);
                    r["in_library"] = json!(in_library);
                    r
                });
                pretty(refs.collect())
            }));
        }
        reply(self.lib.related(&a.id, true).map(|found| {
            let papers = found.iter().map(|c| {
                json!({
                    "title": c.title,
                    "authors": c.authors,
                    "year": c.year,
                    "venue": c.venue,
                    "in_library": c.in_library,
                    "add_with": c.ids.source(),
                })
            });
            pretty(papers.collect())
        }))
    }

    #[tool(
        description = "How a paper connects to the library: the library papers it cites and that cite it, those sharing authors with it, and its references that are not in the library. references_known is false when its references were never looked up (`related` looks them up)."
    )]
    async fn neighbors(&self, Parameters(a): Parameters<IdArgs>) -> Result<CallToolResult, ErrorData> {
        reply(self.lib.graph().neighbors(&a.id).map(|n| pretty(json!(n))))
    }

    #[tool(
        description = "The people behind the library's papers: each with their spellings of their name, their library papers and the people they wrote with most. Spellings count as one person only when they share co-authors or a first name, so two people with the same initials stay apart. Use it to find a person's other papers or close collaborators."
    )]
    async fn authors(&self, Parameters(a): Parameters<AuthorsArgs>) -> Result<CallToolResult, ErrorData> {
        reply(Ok(pretty(json!(self.lib.graph().authors.people(a.name.as_deref())))))
    }

    #[tool(
        description = "Papers missing from the library that its papers (or a project's) cite most, each with the library papers citing it: what to read next. Only papers whose references are known count."
    )]
    async fn suggest(&self, Parameters(a): Parameters<SuggestArgs>) -> Result<CallToolResult, ErrorData> {
        let within = a.project.as_deref().map(|name| self.lib.project(name)).transpose();
        reply(within.map(|within| {
            let found = self.lib.graph().suggestions(within.as_deref());
            pretty(json!(found.into_iter().take(a.limit.unwrap_or(20)).collect::<Vec<_>>()))
        }))
    }

    #[tool(
        description = "Problems in the library (broken or incomplete .bib files, duplicates, papers whose text couldn't be extracted, ...), each with the carrel command that fixes it."
    )]
    async fn doctor(&self) -> Result<CallToolResult, ErrorData> {
        reply(Ok(serde_json::to_string_pretty(&self.lib.doctor()).unwrap_or_default()))
    }

    #[tool(
        description = "Write a summary, keywords or notes for a paper. It is saved as a draft that the user reviews before it becomes part of the library; proposing again replaces the pending draft."
    )]
    async fn propose(&self, Parameters(a): Parameters<ProposeArgs>) -> Result<CallToolResult, ErrorData> {
        reply(
            self.lib
                .propose(&a.id, a.field, &a.content)
                .map(|()| format!("Saved as a {} draft for {}. The user will review it.", a.field.name(), a.id)),
        )
    }

    #[tool(description = "BibTeX entries, keyed by paper id, for the given papers, a project, or the whole library.")]
    async fn bibtex(&self, Parameters(a): Parameters<BibArgs>) -> Result<CallToolResult, ErrorData> {
        let papers = match a.project {
            Some(name) => self.lib.project_papers(&name).map(|(papers, _)| papers),
            None if a.ids.is_empty() => Ok(self.lib.papers().0),
            None => a.ids.iter().map(|id| self.lib.get(id)).collect(),
        };
        reply(papers.map(|ps| ps.iter().map(Paper::bibtex).collect::<Vec<_>>().join("\n")))
    }
}

#[tool_handler]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("carrel", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }
}

pub async fn serve(lib: Library) -> anyhow::Result<()> {
    let server = Server { lib }.serve(stdio()).await?;
    server.waiting().await?;
    Ok(())
}
