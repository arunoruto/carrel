//! Semantic search. A paper's text is cut into passages, and an embedding model on Ollama turns each into
//! a vector; papers whose passages lie close to the query's vector match, whatever words they use. The
//! vectors are kept next to the paper in `embeddings.json`, with the model that made them, so they are
//! computed once (`carrel embed`, or `carrel serve` in its rounds) and travel with the library. The index
//! holds a copy for searching.

use crate::{Field, Library, Paper};
use anyhow::{Context, Result, bail};
use base64::Engine;
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use std::time::Duration;

pub const FILE: &str = "embeddings.json";

/// About this many characters (some 400 tokens) make a passage.
const PASSAGE: usize = 1500;

/// The model used when the config names none and Ollama has it: multilingual, and good with long text.
const PREFERRED: &str = "bge-m3";

/// `[embeddings]` in the config.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Ollama's address, `http://localhost:11434` by default; `OLLAMA_HOST` overrides it.
    pub ollama: Option<String>,
    /// The embedding model. By default bge-m3 when Ollama has it, else the first embedding model it has.
    pub model: Option<String>,
}

/// An embedding model on an Ollama server, found to be there and working.
pub struct Ollama {
    pub url: String,
    pub model: String,
    agent: ureq::Agent,
}

impl Ollama {
    /// Reach Ollama and pick the model. The errors say what to fix: the address, or which model to pull.
    pub fn connect(config: &Config) -> Result<Self> {
        let host = std::env::var("OLLAMA_HOST").ok().filter(|h| !h.trim().is_empty()).or(config.ollama.clone());
        let host = host.unwrap_or_else(|| "http://localhost:11434".into());
        let url = if host.contains("://") { host } else { format!("http://{host}") };
        let url = url.trim_end_matches('/').to_string();
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(5)))
            // The first request loads the model, and a long paper is many passages.
            .timeout_global(Some(Duration::from_secs(600)))
            .http_status_as_error(false)
            // A fresh connection per request: after a long book, a kept one failed every request after it.
            .max_idle_connections(0)
            .build()
            .into();
        let ollama = Self { url, model: String::new(), agent };
        let tags = ollama.call("tags", None).with_context(|| format!("cannot reach Ollama at {}", ollama.url))?;
        let installed: Vec<String> = tags["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| m["name"].as_str())
            .map(|name| name.strip_suffix(":latest").unwrap_or(name).to_string())
            .collect();
        let embeds = |name: &str| {
            let show = ollama.call("show", Some(json!({ "model": name })));
            show.is_ok_and(|s| s["capabilities"].as_array().is_some_and(|c| c.contains(&json!("embedding"))))
        };
        let model = match &config.model {
            Some(wanted) => {
                let wanted = wanted.strip_suffix(":latest").unwrap_or(wanted);
                if !installed.iter().any(|name| name == wanted) {
                    bail!("Ollama at {} doesn't have {wanted}; `ollama pull {wanted}` there", ollama.url);
                }
                if !embeds(wanted) {
                    bail!("{wanted} on Ollama at {} is not an embedding model", ollama.url);
                }
                wanted.to_string()
            }
            None => match installed.iter().find(|name| *name == PREFERRED) {
                Some(name) => name.clone(),
                None => installed.iter().find(|name| embeds(name)).cloned().with_context(|| {
                    format!("Ollama at {} has no embedding model; `ollama pull {PREFERRED}` there", ollama.url)
                })?,
            },
        };
        Ok(Self { model, ..ollama })
    }

    /// One unit-length vector per text, so that a dot product is the cosine similarity.
    pub fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut vectors = Vec::with_capacity(texts.len());
        for batch in texts.chunks(16) {
            let reply = self.call("embed", Some(json!({ "model": self.model, "input": batch })))?;
            let batch_vectors: Vec<Vec<f32>> =
                serde_json::from_value(reply["embeddings"].clone()).context("Ollama sent no embeddings")?;
            if batch_vectors.len() != batch.len() {
                bail!("Ollama sent {} embeddings for {} passages", batch_vectors.len(), batch.len());
            }
            vectors.extend(batch_vectors.into_iter().map(normalized));
        }
        Ok(vectors)
    }

    fn call(&self, endpoint: &str, body: Option<Value>) -> Result<Value> {
        let url = format!("{}/api/{endpoint}", self.url);
        let response = match body {
            Some(body) => self.agent.post(&url).send_json(body),
            None => self.agent.get(&url).call(),
        };
        let mut response = response?;
        let status = response.status();
        let text = response.body_mut().read_to_string()?;
        if !status.is_success() {
            let error = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["error"].as_str().map(String::from));
            bail!("Ollama: {}", error.unwrap_or(text));
        }
        Ok(serde_json::from_str(&text)?)
    }
}

fn normalized(mut v: Vec<f32>) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        v.iter_mut().for_each(|x| *x /= norm);
    }
    v
}

/// A piece of a paper that gets its own vector: the head (title, authors, keywords, abstract, summary)
/// when `offset` is `None`, else `length` characters of `fulltext.md` from character `offset`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub offset: Option<usize>,
    pub length: usize,
}

/// `embeddings.json`.
#[derive(Debug, Serialize, Deserialize)]
pub struct Stored {
    /// The model that made the vectors; vectors of different models can't be compared.
    pub model: String,
    /// A hash of the text the vectors were made from, to tell when they're out of date.
    pub source: String,
    pub passages: Vec<Span>,
    /// One vector per passage, all concatenated, as little-endian f32 in base64.
    pub vectors: String,
}

impl Stored {
    pub fn vectors(&self) -> Result<Vec<Vec<f32>>> {
        let bytes = base64::engine::general_purpose::STANDARD.decode(&self.vectors)?;
        let floats: Vec<f32> = bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect();
        if self.passages.is_empty() || !floats.len().is_multiple_of(self.passages.len()) {
            bail!("{FILE}: {} numbers don't split into {} vectors", floats.len(), self.passages.len());
        }
        Ok(floats.chunks(floats.len() / self.passages.len()).map(<[f32]>::to_vec).collect())
    }
}

/// What gets embedded: the head, then the full text in passages cut at paragraphs where possible.
pub fn passages(p: &Paper) -> Vec<(Span, String)> {
    let b = &p.bib;
    let head = [b.title(), b.authors().join("; "), b.keywords().join(", ")]
        .into_iter()
        .chain(b.get("abstract"))
        .chain(p.get(Field::Summary))
        .filter(|s| !s.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut all = vec![(Span { offset: None, length: head.chars().count() }, head)];
    let text: Vec<char> = p.fulltext().unwrap_or_default().chars().collect();
    let mut start = 0;
    while start < text.len() {
        let end = (start + PASSAGE).min(text.len());
        // The last paragraph break in the passage's second half, else the last line break, else space.
        let cut = if end == text.len() {
            end
        } else {
            let window = &text[start + PASSAGE / 2..end];
            let after = |pos: Option<usize>, skip| pos.map(|i| start + PASSAGE / 2 + i + skip);
            let paragraph = window.windows(2).rposition(|w| w == ['\n', '\n']);
            after(paragraph, 2)
                .or_else(|| after(window.iter().rposition(|&c| c == '\n'), 1))
                .or_else(|| after(window.iter().rposition(|&c| c == ' '), 1))
                .unwrap_or(end)
        };
        let piece: String = text[start..cut].iter().collect();
        if !piece.trim().is_empty() {
            all.push((Span { offset: Some(start), length: cut - start }, piece));
        }
        start = cut;
    }
    all
}

fn source(passages: &[(Span, String)]) -> String {
    let mut hash = Md5::new();
    for (_, text) in passages {
        hash.update(text.as_bytes());
        hash.update([0]);
    }
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// A paper's stored embeddings, if it has readable ones.
pub fn stored(p: &Paper) -> Option<Stored> {
    serde_json::from_str(&fs::read_to_string(p.dir.join(FILE)).ok()?).ok()
}

impl Library {
    /// Whether a paper's embeddings were made by `model` from its current text.
    pub fn embedded(&self, p: &Paper, model: &str) -> bool {
        stored(p).is_some_and(|s| s.model == model && s.source == source(&passages(p)))
    }

    /// Compute a paper's embeddings unless they're up to date. Returns whether it did.
    pub fn embed(&self, ollama: &Ollama, p: &Paper) -> Result<bool> {
        let passages = passages(p);
        let source = source(&passages);
        if stored(p).is_some_and(|s| s.model == ollama.model && s.source == source) {
            return Ok(false);
        }
        let texts: Vec<String> = passages.iter().map(|(_, text)| text.clone()).collect();
        let vectors = ollama.embed(&texts)?;
        let bytes: Vec<u8> = vectors.iter().flatten().flat_map(|x| x.to_le_bytes()).collect();
        let stored = Stored {
            model: ollama.model.clone(),
            source,
            passages: passages.into_iter().map(|(span, _)| span).collect(),
            vectors: base64::engine::general_purpose::STANDARD.encode(bytes),
        };
        // Written aside and renamed, so a search never reads half a file.
        let tmp = p.dir.join(format!(".{FILE}.tmp"));
        fs::write(&tmp, serde_json::to_string(&stored)?)?;
        fs::rename(&tmp, p.dir.join(FILE))?;
        Ok(true)
    }

    /// For each embedded paper, the `k` others most alike in title, abstract and keywords, with the cosine
    /// similarity, best first. Works from the stored vectors alone, without Ollama.
    pub fn similar(&self, k: usize) -> Result<std::collections::HashMap<String, Vec<(String, f32)>>> {
        let papers = self.papers().0;
        let model = self.embeddings.as_ref().and_then(|c| c.model.as_deref());
        let Some((_, heads)) = crate::index::heads(&self.index, &papers, model)? else { return Ok(Default::default()) };
        // ponytail: every pair is compared, fine for thousands of papers.
        let similar = heads.iter().map(|(id, v)| {
            let mut scores: Vec<(String, f32)> = heads
                .iter()
                .filter(|(other, _)| *other != id)
                .map(|(other, w)| (other.clone(), v.iter().zip(w).map(|(a, b)| a * b).sum()))
                .collect();
            scores.sort_by(|a, b| b.1.total_cmp(&a.1));
            scores.truncate(k);
            (id.clone(), scores)
        });
        Ok(similar.collect())
    }

    /// Papers whose passages are closest in meaning to `query`, best first, each with its closest passage.
    /// `project:name` searches within a project. Papers without embeddings by `ollama`'s model are missed.
    pub fn semantic_search(&self, ollama: &Ollama, query: &str, limit: usize) -> Result<Vec<(Paper, String)>> {
        let (words, allowed) = self.scope(query)?;
        let wanted = |p: &Paper| allowed.as_ref().is_none_or(|ids| ids.contains(&p.id));
        if words.is_empty() {
            bail!("search for what?");
        }
        let vector = ollama.embed(&[words])?.remove(0);
        let papers = self.papers().0;
        let hits = crate::index::nearest(&self.index, &papers, &ollama.model, &vector)?;
        let mut by_id: std::collections::HashMap<String, Paper> =
            papers.into_iter().filter(|p| wanted(p)).map(|p| (p.id.clone(), p)).collect();
        let found = hits.into_iter().filter_map(|(id, span, score)| {
            let p = by_id.remove(&id)?;
            let snippet = snippet(&p, &span, score);
            Some((p, snippet))
        });
        Ok(found.take(limit).collect())
    }
}

/// The start of the passage that matched, with how well, and where it is for reading on.
fn snippet(p: &Paper, span: &Span, score: f32) -> String {
    let (place, text) = match span.offset {
        None => ("title and abstract".to_string(), passages(p).swap_remove(0).1),
        Some(offset) => (
            format!("full text at {offset}"),
            p.fulltext().unwrap_or_default().chars().skip(offset).take(span.length).collect(),
        ),
    };
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let short: String = text.chars().take(200).collect();
    let more = if short.len() < text.len() { "…" } else { "" };
    format!("{score:.2} {place}: {short}{more}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cutting_passages() {
        let tmp = std::env::temp_dir().join(format!("carrel-passages-{}", std::process::id()));
        let lib = Library::open(&tmp).unwrap();
        fs::create_dir_all(tmp.join("p")).unwrap();
        fs::write(tmp.join("p/p.bib"), "@article{p,\n  title = {On Things},\n}\n").unwrap();
        let paragraph = "word ".repeat(100); // 500 characters
        let text = format!("{paragraph}\n\n{paragraph}\n\n{paragraph}\n\n{}", "x".repeat(4000));
        fs::write(tmp.join("p/fulltext.md"), &text).unwrap();
        let p = lib.get("p").unwrap();

        let all = passages(&p);
        assert_eq!(all[0], (Span { offset: None, length: 9 }, "On Things".to_string()));
        let chars: Vec<char> = text.chars().collect();
        let mut next = 0;
        for (span, piece) in &all[1..] {
            let offset = span.offset.unwrap();
            assert_eq!(offset, next, "passages follow each other");
            assert!(span.length <= PASSAGE);
            assert_eq!(piece, &chars[offset..offset + span.length].iter().collect::<String>());
            next = offset + span.length;
        }
        assert_eq!(next, chars.len(), "and cover the whole text");
        assert!(all[1].1.ends_with("\n\n"), "the first is cut at a paragraph: {:?}", all[1].1);
        assert_eq!(all[3].1, "x".repeat(PASSAGE), "text without breaks is cut anywhere");
        fs::remove_dir_all(tmp).unwrap();
    }
}
