//! Papers recommended to the user, for now from Google Scholar's alert emails (saved as `.eml` files, for
//! example by a mail filter). They wait in `recommended/` until the user adds or dismisses them, so search,
//! the map and bibliographies only see papers someone chose. With embeddings, each is scored by how alike
//! it is to the library.

use crate::{Library, Staging, fetch};
use anyhow::{Context, Result, bail};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// The folder in the library that holds them, one JSON file each.
pub const DIR: &str = "recommended";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Recommendation {
    /// Its file name, from the title.
    pub key: String,
    pub title: String,
    /// As the alert writes them (`G Liu`); Scholar leaves out the rest of a long list.
    pub authors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub venue: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<i32>,
    /// The start of its abstract.
    #[serde(default)]
    pub snippet: String,
    /// Where the alert links to: the publisher's page, arXiv, ADS, or the PDF itself.
    pub url: String,
    /// Whether `url` is the PDF, as Scholar marks it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pdf: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doi: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arxiv: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bibcode: Option<String>,
    /// The days alerts recommended it, oldest first: more days, more often recommended.
    #[serde(default)]
    pub dates: Vec<String>,
    /// Which alerts (their subjects: `Recommended articles`, `new citations to my articles`, ...).
    #[serde(default)]
    pub alerts: Vec<String>,
    /// How alike to the library, the mean cosine similarity to its three closest papers; with embeddings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dismissed: bool,
}

/// What an import found.
#[derive(Debug, Default)]
pub struct Imported {
    /// The alerts read.
    pub alerts: usize,
    /// New recommendations, best first.
    pub new: Vec<Recommendation>,
}

impl Library {
    /// The recommendations not dismissed (with `all`, every one), best first: by score, then by how often
    /// they were recommended, then newest.
    pub fn recommendations(&self, all: bool) -> Vec<Recommendation> {
        let dir = self.root.join(DIR);
        let files = fs::read_dir(&dir).into_iter().flatten().flatten();
        let mut found: Vec<Recommendation> = files
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| serde_json::from_str(&fs::read_to_string(e.path()).ok()?).ok())
            .filter(|r: &Recommendation| all || !r.dismissed)
            .collect();
        sort(&mut found);
        found
    }

    pub fn recommendation(&self, key: &str) -> Result<Recommendation> {
        if !crate::valid_id(key) {
            bail!("invalid recommendation {key:?}");
        }
        let path = self.root.join(DIR).join(format!("{key}.json"));
        let text = fs::read_to_string(&path).with_context(|| format!("no recommendation {key:?}"))?;
        serde_json::from_str(&text).with_context(|| format!("{} is invalid", path.display()))
    }

    pub fn save_recommendation(&self, r: &Recommendation) -> Result<()> {
        fs::create_dir_all(self.root.join(DIR))?;
        fs::write(self.root.join(DIR).join(format!("{}.json", r.key)), serde_json::to_string_pretty(r)? + "\n")?;
        Ok(())
    }

    /// Read alert emails (`.eml` files, or folders of them). Papers already in the library are left out;
    /// ones recommended before get the new date. Other emails are skipped. New ones are scored when
    /// embeddings are set up and Ollama answers.
    pub fn import_alerts(&self, paths: &[PathBuf]) -> Result<Imported> {
        let mut files = vec![];
        for path in paths {
            match path.is_dir() {
                true => files.extend(
                    fs::read_dir(path)?
                        .flatten()
                        .map(|e| e.path())
                        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("eml"))),
                ),
                false => files.push(path.clone()),
            }
        }
        files.sort();
        let library: Vec<String> = self.papers().0.iter().map(|p| fetch::title_key(&p.bib.title())).collect();
        let mut imported = Imported::default();
        let mut new: Vec<Recommendation> = vec![];
        for file in files {
            let raw = fs::read(&file).with_context(|| format!("cannot read {}", file.display()))?;
            let Some(mut alert) = parse_alert(&raw) else { continue };
            imported.alerts += 1;
            for mut r in std::mem::take(&mut alert.items) {
                if library.contains(&fetch::title_key(&r.title)) {
                    continue;
                }
                // Recommended again: one more day, in this import or an earlier one.
                if let Some(pending) = new.iter_mut().find(|n| n.key == r.key) {
                    seen_in(pending, &alert);
                    continue;
                }
                if let Ok(mut old) = self.recommendation(&r.key) {
                    if seen_in(&mut old, &alert) {
                        self.save_recommendation(&old)?;
                    }
                    continue;
                }
                seen_in(&mut r, &alert);
                new.push(r);
            }
        }
        if let Err(e) = self.score(&mut new) {
            eprintln!("not scored: {e:#}");
        }
        for r in &new {
            self.save_recommendation(r)?;
        }
        sort(&mut new);
        imported.new = new;
        Ok(imported)
    }

    /// Score recommendations by how alike they are to the library's papers (their titles, abstracts and
    /// keywords, as embedded for semantic search).
    pub fn score(&self, recs: &mut [Recommendation]) -> Result<()> {
        let Some(config) = &self.embeddings else { return Ok(()) };
        if recs.is_empty() {
            return Ok(());
        }
        let ollama = crate::embed::Ollama::connect(config)?;
        let papers = self.papers().0;
        let Some((_, heads)) = crate::index::heads(&self.index, &papers, Some(&ollama.model))? else { return Ok(()) };
        if heads.is_empty() {
            return Ok(());
        }
        let texts: Vec<String> = recs.iter().map(|r| format!("{}\n\n{}", r.title, r.snippet)).collect();
        for (r, v) in recs.iter_mut().zip(ollama.embed(&texts)?) {
            let mut sims: Vec<f32> = heads.values().map(|w| v.iter().zip(w).map(|(a, b)| a * b).sum()).collect();
            sims.sort_by(|a, b| b.total_cmp(a));
            let top = &sims[..sims.len().min(3)];
            r.score = Some(top.iter().sum::<f32>() / top.len() as f32);
        }
        Ok(())
    }

    pub fn dismiss(&self, key: &str) -> Result<()> {
        let mut r = self.recommendation(key)?;
        r.dismissed = true;
        self.save_recommendation(&r)
    }

    /// Add a recommendation to the library: by its arXiv id, ADS bibcode or DOI when the link has one (and
    /// the DOI's title is the same), else by its title. When the link is a PDF, it comes along if it can be
    /// downloaded. Returns the new paper's id and messages worth showing.
    pub fn add_recommended(&self, key: &str) -> Result<(String, Vec<String>)> {
        let r = self.recommendation(key)?;
        let mut messages = vec![];
        let tmp = Staging::new(&self.root)?;
        if r.pdf && r.arxiv.is_none() {
            let pdf = tmp.0.join("paper.pdf");
            let got = fetch::download(&r.url, &pdf).and_then(|()| match fs::read(&pdf)?.starts_with(b"%PDF") {
                true => Ok(()),
                false => bail!("not a PDF; it may want a login"),
            });
            if let Err(e) = got {
                let _ = fs::remove_file(&pdf);
                messages.push(format!("without its PDF: {} gave none ({})", host(&r.url), e.root_cause()));
            }
        }
        let none = fetch::Ids::default();
        let mut known = None;
        let mut ids = match (&r.arxiv, &r.doi) {
            (Some(arxiv), _) => fetch::Ids { arxiv: Some(arxiv.clone()), ..none.clone() },
            // A DOI in a link may be cut short or have more after it (`/meta`, `/5453393`): only when its
            // title is this one.
            (None, Some(doi)) => {
                let ids = fetch::Ids { doi: Some(doi.clone()), ..none.clone() };
                match fetch::metadata(&ids) {
                    Ok(entry) if crate::update::same_work(&r.title, &entry.title()) => ids,
                    _ => none.clone(),
                }
            }
            _ => none.clone(),
        };
        if ids.is_empty()
            && let Some(bibcode) = &r.bibcode
            && let Ok(Some(ads)) = crate::refs::ads()
            && let Ok(entry) = ads.bibtex(bibcode)
        {
            match entry.doi() {
                Some(doi) => ids.doi = Some(doi),
                None => known = Some(entry),
            }
        }
        if ids.is_empty() && known.is_none() {
            ids = fetch::by_title(&r.title).with_context(|| {
                format!("can't tell which paper it is; add it by its DOI instead (its page: {})", r.url)
            })?;
        }
        let (id, more) = self.add_staged(tmp, ids, known, &r.url, false)?;
        messages.extend(more);
        let _ = fs::remove_file(self.root.join(DIR).join(format!("{key}.json")));
        Ok((id, messages))
    }
}

/// Note that `alert` recommended it; false when it was noted already.
fn seen_in(r: &mut Recommendation, alert: &Alert) -> bool {
    let new = !r.dates.contains(&alert.date) || !r.alerts.contains(&alert.subject);
    if !r.dates.contains(&alert.date) {
        r.dates.push(alert.date.clone());
        r.dates.sort();
    }
    if !r.alerts.contains(&alert.subject) {
        r.alerts.push(alert.subject.clone());
    }
    new
}

fn sort(recs: &mut [Recommendation]) {
    recs.sort_by(|a, b| {
        let score = |r: &Recommendation| r.score.unwrap_or(f32::NEG_INFINITY);
        score(b)
            .total_cmp(&score(a))
            .then(b.dates.len().cmp(&a.dates.len()))
            .then(b.dates.last().cmp(&a.dates.last()))
            .then(a.key.cmp(&b.key))
    });
}

fn host(url: &str) -> &str {
    url.split("://").nth(1).and_then(|rest| rest.split('/').next()).unwrap_or(url)
}

/// One alert email: when it came, its subject, and the papers it recommends.
#[derive(Debug)]
pub struct Alert {
    pub date: String,
    pub subject: String,
    pub items: Vec<Recommendation>,
}

/// A Google Scholar alert, from the raw email; None for any other email.
pub fn parse_alert(raw: &[u8]) -> Option<Alert> {
    let (headers, body) = split_message(raw);
    let from = header(&headers, "from")?;
    if !from.to_lowercase().contains("scholaralerts-noreply@google.com") {
        return None;
    }
    let html = html_part(&headers, body)?;
    let date = header(&headers, "date").and_then(|d| day(&d)).unwrap_or_default();
    let subject = header(&headers, "subject").map(|s| decode_words(&s)).unwrap_or_default();
    Some(Alert { date, subject, items: items(&html) })
}

/// The papers in an alert's HTML: a title linking to it (through scholar_url), maybe marked [PDF], then a
/// line of authors, venue and year, then the start of its abstract.
fn items(html: &str) -> Vec<Recommendation> {
    let marker = "class=\"gse_alrt_title\"";
    let starts: Vec<usize> = html.match_indices(marker).map(|(i, _)| i).collect();
    let mut found = vec![];
    for (n, &at) in starts.iter().enumerate() {
        let end = starts.get(n + 1).copied().unwrap_or(html.len());
        // The <h3> holding the link, from its start: the [PDF] mark is before the link.
        let h3 = html[..at].rfind("<h3").unwrap_or(at);
        let Some(a) = html[h3..at].rfind("<a ").map(|i| h3 + i) else { continue };
        let tag = &html[a..at + marker.len()];
        // A paper's link goes through scholar_url; the footer's "See all recommendations" doesn't.
        let Some(href) = attr(tag, "href").filter(|h| h.contains("scholar_url?")) else { continue };
        let Some(text_end) = html[at..end].find("</a>").map(|i| at + i) else { continue };
        let text_start = html[at..text_end].find('>').map_or(text_end, |i| at + i + 1);
        let title = fetch::strip_tags(&html[text_start..text_end]);
        if title.is_empty() {
            continue;
        }
        let url = unwrap_scholar(&fetch::strip_tags(&href));
        let rest = &html[text_end..end];
        let divs: Vec<&str> = rest.split("<div").skip(1).collect();
        let byline = divs.first().map(|d| fetch::strip_tags(&format!("<div{d}"))).unwrap_or_default();
        let snippet = divs
            .iter()
            .find(|d| d.contains("gse_alrt_sni"))
            .map(|d| fetch::strip_tags(&format!("<div{d}")))
            .unwrap_or_default();
        let (authors, venue, year) = byline_parts(&byline);
        let mut r = Recommendation {
            key: key(&title),
            title,
            authors,
            venue,
            year,
            snippet,
            pdf: html[h3..a].contains("[PDF]"),
            ..Default::default()
        };
        match fetch::classify(&url) {
            Ok(fetch::Source::Arxiv(id)) => r.arxiv = Some(id),
            Ok(fetch::Source::Doi(doi)) => r.doi = Some(doi),
            Ok(fetch::Source::Bibcode(b)) => r.bibcode = Some(b),
            _ => {}
        }
        r.url = url;
        found.push(r);
    }
    found
}

/// `G Liu, C Zhang… - Journal of Earth Science, 2026` into authors, venue and year; either half after the
/// dash may be missing, and Scholar cuts long lists and venues short with `…`.
fn byline_parts(line: &str) -> (Vec<String>, Option<String>, Option<i32>) {
    let line = line.replace('\u{a0}', " ");
    let (authors, rest) = line.split_once(" - ").unwrap_or((&line, ""));
    let authors = authors.trim().trim_end_matches('…').split(", ").map(|a| a.trim().to_string());
    let authors = authors.filter(|a| !a.is_empty()).collect();
    let rest = rest.trim();
    let year_at = |s: &str| s.parse::<i32>().ok().filter(|y| (1000..3000).contains(y));
    let (venue, year) = match rest.rsplit_once(", ") {
        Some((venue, y)) if year_at(y).is_some() => (venue, year_at(y)),
        _ if year_at(rest).is_some() => ("", year_at(rest)),
        _ => (rest, None),
    };
    (authors, Some(venue.trim().to_string()).filter(|v| !v.is_empty()), year)
}

/// A file name from a title: its words, lowercase ASCII, joined by dashes, at most about 60 characters.
fn key(title: &str) -> String {
    let words = deunicode::deunicode(title).to_lowercase();
    let mut key = String::new();
    for word in words.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()) {
        if key.len() + word.len() > 60 && !key.is_empty() {
            break;
        }
        if !key.is_empty() {
            key.push('-');
        }
        key.push_str(&word[..word.len().min(60)]);
    }
    if key.is_empty() { "untitled".into() } else { key }
}

/// Scholar's links go through `scholar.google.com/scholar_url?url=<the link>&...`.
fn unwrap_scholar(href: &str) -> String {
    let query = href.split_once("scholar_url?").map(|(_, q)| q);
    let inner = query.and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("url=")));
    inner.map_or(href.to_string(), percent_decode)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let (mut out, mut i) = (vec![], 0);
    while i < b.len() {
        let hex = |j: usize| b.get(j).and_then(|c| (*c as char).to_digit(16));
        match (b[i], hex(i + 1), hex(i + 2)) {
            (b'%', Some(h), Some(l)) => {
                out.push((h * 16 + l) as u8);
                i += 3;
            }
            (c, ..) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let start = tag.find(&format!("{name}=\""))? + name.len() + 2;
    Some(tag[start..].split('"').next()?.to_string())
}

/// Headers (unfolded, as name and value) and the body.
fn split_message(raw: &[u8]) -> (Vec<(String, String)>, &[u8]) {
    let end = raw.windows(4).position(|w| w == b"\r\n\r\n").map(|i| (i, i + 4));
    let end = end.or_else(|| raw.windows(2).position(|w| w == b"\n\n").map(|i| (i, i + 2)));
    let (head, body) = end.map_or((raw, &raw[raw.len()..]), |(h, b)| (&raw[..h], &raw[b..]));
    let mut headers: Vec<(String, String)> = vec![];
    for line in String::from_utf8_lossy(head).lines() {
        match (line.starts_with([' ', '\t']), headers.last_mut()) {
            (true, Some((_, value))) => {
                value.push(' ');
                value.push_str(line.trim());
            }
            _ => {
                if let Some((name, value)) = line.split_once(':') {
                    headers.push((name.trim().to_lowercase(), value.trim().to_string()));
                }
            }
        }
    }
    (headers, body)
}

fn header(headers: &[(String, String)], name: &str) -> Option<String> {
    headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone())
}

/// The text/html part of a message, decoded; in a multipart message, the first one found.
fn html_part(headers: &[(String, String)], body: &[u8]) -> Option<String> {
    let kind = header(headers, "content-type").unwrap_or_default();
    let lower = kind.to_lowercase();
    if lower.starts_with("multipart/") {
        let boundary = kind.split(';').find_map(|p| {
            let (k, v) = p.trim().split_once('=')?;
            k.eq_ignore_ascii_case("boundary").then(|| v.trim_matches('"').to_string())
        })?;
        let delimiter = format!("--{boundary}");
        let text = body;
        let mut parts = vec![];
        let mut rest = text;
        while let Some(i) = find(rest, delimiter.as_bytes()) {
            parts.push(&rest[..i]);
            rest = &rest[i + delimiter.len()..];
        }
        return parts.into_iter().skip(1).find_map(|part| {
            let part = part.strip_prefix(b"\r\n").or_else(|| part.strip_prefix(b"\n")).unwrap_or(part);
            let (h, b) = split_message(part);
            html_part(&h, b)
        });
    }
    if !lower.starts_with("text/html") {
        return None;
    }
    let bytes = match header(headers, "content-transfer-encoding").unwrap_or_default().to_lowercase().as_str() {
        "quoted-printable" => quoted_printable(body),
        "base64" => {
            let clean: Vec<u8> = body.iter().copied().filter(|c| !c.is_ascii_whitespace()).collect();
            base64::engine::general_purpose::STANDARD.decode(clean).ok()?
        }
        _ => body.to_vec(),
    };
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn quoted_printable(body: &[u8]) -> Vec<u8> {
    let (mut out, mut i) = (vec![], 0);
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    while i < body.len() {
        if body[i] == b'=' {
            match (body.get(i + 1), body.get(i + 2)) {
                (Some(b'\r'), Some(b'\n')) => i += 3,
                (Some(b'\n'), _) => i += 2,
                (Some(&h), Some(&l)) if hex(h).is_some() && hex(l).is_some() => {
                    out.push(hex(h).unwrap() * 16 + hex(l).unwrap());
                    i += 3;
                }
                _ => {
                    out.push(b'=');
                    i += 1;
                }
            }
        } else {
            out.push(body[i]);
            i += 1;
        }
    }
    out
}

/// A header's `=?UTF-8?Q?...?=` and `=?UTF-8?B?...?=` words, decoded.
fn decode_words(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("=?") {
        let Some(parts) = rest[start + 2..].splitn(3, '?').collect::<Vec<_>>().get(..3).map(|p| p.to_vec()) else {
            break;
        };
        let Some(end) = parts[2].find("?=") else { break };
        let text = &parts[2][..end];
        let decoded = match parts[1].to_ascii_uppercase().as_str() {
            "Q" => Some(quoted_printable(text.replace('_', " ").as_bytes())),
            "B" => base64::engine::general_purpose::STANDARD.decode(text).ok(),
            _ => None,
        };
        let Some(decoded) = decoded else { break };
        // Whitespace between two encoded words isn't text.
        let before = &rest[..start];
        if !(before.trim().is_empty() && !out.is_empty()) {
            out.push_str(before);
        }
        out.push_str(&String::from_utf8_lossy(&decoded));
        let consumed = start + 2 + parts[0].len() + 1 + parts[1].len() + 1 + end + 2;
        rest = &rest[consumed..];
    }
    out.push_str(rest);
    out
}

/// `Sat, 24 Oct 2026 23:24:09 -0700` as `2026-10-24`, the day as the sender wrote it.
fn day(date: &str) -> Option<String> {
    const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let words: Vec<&str> = date.split([' ', ',']).filter(|w| !w.is_empty()).collect();
    let m = words.iter().position(|w| MONTHS.contains(&w.to_lowercase().get(..3).unwrap_or("")))?;
    let month = MONTHS.iter().position(|x| words[m].to_lowercase().starts_with(x))? + 1;
    let day: u32 = words.get(m.checked_sub(1)?)?.parse().ok()?;
    let year: u32 = words.get(m + 1)?.parse().ok()?;
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

/// The `.eml` files directly in these folders.
pub fn alert_files(folders: &[PathBuf]) -> Vec<PathBuf> {
    folders
        .iter()
        .flat_map(|dir| fs::read_dir(dir).into_iter().flatten().flatten())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("eml")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alert() -> Vec<u8> {
        let html = "<div><h3 style=3D\"font-weight:normal;margin:0\"><span>[PDF]</span> <a href=3D\"https://sch=\nolar.google.com/scholar_url?url=3Dhttps://arxiv.org/pdf/2610.29273&amp;hl=3Den\" class=3D\"gse_al=\nrt_title\">B-fields &amp; <b>Stars</b></a></h3><div style=3D\"color:#006621\">RS Furuya, D Berry=E2=80=A6=C2=\n=A0- arXiv preprint arXiv =E2=80=A6, 2026</div><div class=3D\"gse_alrt_sni\">Magnetic <br>fields =E2=80=A6</=\ndiv>\n<h3 style=3D\"font-weight:normal;margin:0\"><a href=3D\"https://scholar.google.com/scholar_url?url=3D=\nhttps://iopscience.iop.org/article/10.3847/1538-4357/aeaa5c/meta&amp;hl=3Den\" class=3D\"gse_alrt_title\">S=\nublimation of H2O</a></h3><div style=3D\"color:#006621\">E Zubko - 2025</div><div class=3D\"gse_alrt_sni\">Comets</div><h3><a href=3D\"https://scholar.google.com/citations?view_op=3Dlist\" class=3D\"gse_alrt_title\">See all recommendations</a></h3><div>footer</div>";
        format!(
            "MIME-Version: 1.0\r\nDate: Sat, 24 Oct 2026 23:24:09 -0700\r\nSubject: =?UTF-8?Q?Recommended_articles?=\r\nFrom: Google Scholar Alerts <scholaralerts-noreply@google.com>\r\nContent-Type: text/html; charset=\"UTF-8\"\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\n{}",
            html.replace('\n', "\r\n")
        )
        .into_bytes()
    }

    #[test]
    fn alerts() {
        let a = parse_alert(&alert()).unwrap();
        assert_eq!((a.date.as_str(), a.subject.as_str()), ("2026-10-24", "Recommended articles"));
        assert_eq!(a.items.len(), 2);
        let first = &a.items[0];
        assert_eq!(first.title, "B-fields & Stars");
        assert_eq!(first.key, "b-fields-stars");
        assert_eq!(first.url, "https://arxiv.org/pdf/2610.29273");
        assert_eq!((first.pdf, first.arxiv.as_deref()), (true, Some("2610.29273")));
        assert_eq!(first.authors, ["RS Furuya", "D Berry"]);
        assert_eq!((first.venue.as_deref(), first.year), (Some("arXiv preprint arXiv …"), Some(2026)));
        assert_eq!(first.snippet, "Magnetic fields …");
        let second = &a.items[1];
        assert_eq!((second.pdf, second.venue.as_deref(), second.year), (false, None, Some(2025)));
        assert!(second.doi.as_deref().is_some_and(|d| d.starts_with("10.3847/1538-4357/aeaa5c")));

        // Other email, and a multipart one with the alert inside.
        assert!(parse_alert(b"From: someone@example.org\r\nContent-Type: text/html\r\n\r\n<a>").is_none());
        let html = String::from_utf8(alert()).unwrap();
        let (head, body) = html.split_once("\r\n\r\n").unwrap();
        let head = head.replace(
            "Content-Type: text/html; charset=\"UTF-8\"\r\nContent-Transfer-Encoding: quoted-printable",
            "Content-Type: multipart/alternative; boundary=\"b1\"",
        );
        let multi = format!(
            "{head}\r\n\r\n--b1\r\nContent-Type: text/plain\r\n\r\nplain\r\n--b1\r\nContent-Type: text/html; charset=UTF-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\n{body}\r\n--b1--\r\n"
        );
        assert_eq!(parse_alert(multi.as_bytes()).unwrap().items.len(), 2);
    }

    #[test]
    fn bylines() {
        let parts = |s| byline_parts(s);
        assert_eq!(
            parts("S Sugita - Earth, Planets and Space"),
            (vec!["S Sugita".into()], Some("Earth, Planets and Space".into()), None)
        );
        assert_eq!(parts("B Scott"), (vec!["B Scott".into()], None, None));
        assert_eq!(parts("R Canu-Blot, M Wieser… - 2024").1, None);
        assert_eq!(
            parts("A Morse - European Lunar Symposium 2025").1.as_deref(),
            Some("European Lunar Symposium 2025")
        );
        assert_eq!(decode_words("=?UTF-8?Q?W=C3=B6hler_-_new?= =?UTF-8?B?YXJ0aWNsZXM=?="), "Wöhler - newarticles");
        assert_eq!(day("24 Oct 2026 23:24:09 +0000").as_deref(), Some("2026-10-24"));
        assert_eq!(key("Ünïcode: a  title!"), "unicode-a-title");
    }

    #[test]
    fn importing() {
        let tmp = std::env::temp_dir().join(format!("carrel-recommended-{}", std::process::id()));
        let lib = Library::open(tmp.join("lib")).unwrap();
        fs::create_dir_all(tmp.join("mail")).unwrap();
        fs::write(tmp.join("mail/a.eml"), alert()).unwrap();
        fs::write(tmp.join("mail/b.eml"), String::from_utf8(alert()).unwrap().replace("24 Oct", "31 Oct")).unwrap();
        fs::write(tmp.join("mail/other.eml"), "From: x@y.z\r\n\r\nhi").unwrap();
        // One of them is in the library already.
        fs::create_dir_all(lib.root.join("zubko2025")).unwrap();
        fs::write(lib.root.join("zubko2025/zubko2025.bib"), "@misc{zubko2025, title = {Sublimation of H$_2$O}}")
            .unwrap();

        let imported = lib.import_alerts(&[tmp.join("mail")]).unwrap();
        assert_eq!((imported.alerts, imported.new.len()), (2, 1));
        assert_eq!(lib.recommendations(false)[0].dates, ["2026-10-24", "2026-10-31"], "recommended twice");
        assert!(lib.import_alerts(&[tmp.join("mail")]).unwrap().new.is_empty(), "nothing new the second time");
        assert_eq!(lib.recommendations(false)[0].dates.len(), 2);
        assert!(!lib.folders().contains(&DIR.to_string()), "not a paper");

        lib.dismiss("b-fields-stars").unwrap();
        assert!(lib.recommendations(false).is_empty());
        assert_eq!(lib.recommendations(true).len(), 1);
        assert!(lib.import_alerts(&[tmp.join("mail")]).unwrap().new.is_empty(), "dismissed stays dismissed");
        assert!(lib.recommendation("../x").is_err());
        fs::remove_dir_all(tmp).unwrap();
    }
}
