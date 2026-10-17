//! Projects: named lists of papers, such as everything a thesis chapter cites. Each one is a text file
//! `<library>/projects/<name>.txt` with a paper id per line (`#` starts a comment), so it can be
//! edited by hand and turned into a bibliography with `carrel bib --project <name>`.

use crate::{Library, Paper, valid_id};
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

/// The folder holding the project files, inside the library.
pub const DIR: &str = "projects";

impl Library {
    fn project_path(&self, name: &str) -> Result<PathBuf> {
        if !valid_id(name) {
            bail!("invalid project name {name:?}");
        }
        Ok(self.root.join(DIR).join(format!("{name}.txt")))
    }

    /// Project names, sorted.
    pub fn projects(&self) -> Vec<String> {
        let files = fs::read_dir(self.root.join(DIR)).into_iter().flatten().flatten().map(|e| e.path());
        let mut names: Vec<String> = files
            .filter(|p| p.extension().is_some_and(|e| e == "txt"))
            .filter_map(|p| Some(p.file_stem()?.to_string_lossy().into_owned()))
            .collect();
        names.sort();
        names
    }

    /// The paper ids in a project, in file order.
    pub fn project(&self, name: &str) -> Result<Vec<String>> {
        let text = fs::read_to_string(self.project_path(name)?).with_context(|| format!("no project {name:?}"))?;
        Ok(text.lines().map(line_id).filter(|l| !l.is_empty()).map(String::from).collect())
    }

    /// A project's papers in file order, plus the ids it lists that aren't in the library.
    pub fn project_papers(&self, name: &str) -> Result<(Vec<Paper>, Vec<String>)> {
        let (mut papers, mut missing) = (vec![], vec![]);
        for id in self.project(name)? {
            match self.get(&id) {
                Ok(p) => papers.push(p),
                Err(_) => missing.push(id),
            }
        }
        Ok((papers, missing))
    }

    /// Add papers to a project, creating it if needed. The file's other lines are kept as they are.
    pub fn add_to_project(&self, name: &str, ids: &[String]) -> Result<()> {
        let path = self.project_path(name)?;
        let mut listed = if path.exists() { self.project(name)? } else { vec![] };
        let mut text = fs::read_to_string(&path).unwrap_or_default();
        for id in ids {
            if listed.contains(id) {
                continue;
            }
            self.get(id)?;
            listed.push(id.clone());
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text += &format!("{id}\n");
        }
        fs::create_dir_all(self.root.join(DIR))?;
        fs::write(path, text)?;
        Ok(())
    }

    pub fn remove_from_project(&self, name: &str, ids: &[String]) -> Result<()> {
        self.project(name)?;
        let path = self.project_path(name)?;
        let text = fs::read_to_string(&path)?;
        let kept: Vec<&str> = text.lines().filter(|l| !ids.iter().any(|id| line_id(l) == id)).collect();
        fs::write(path, kept.join("\n") + "\n")?;
        Ok(())
    }

    /// Delete a project; its papers stay in the library.
    pub fn delete_project(&self, name: &str) -> Result<()> {
        fs::remove_file(self.project_path(name)?).with_context(|| format!("no project {name:?}"))
    }

    /// Put every library paper that the given LaTeX (.tex) or Typst (.typ) files cite into a project,
    /// creating it if needed. Returns the ids added, the cited keys missing from the library, and the
    /// project's papers that no file cites.
    pub fn sync_project(
        &self,
        name: &str,
        files: &[impl AsRef<Path>],
    ) -> Result<(Vec<String>, Vec<String>, Vec<String>)> {
        let mut cited: Vec<String> = vec![];
        for file in files {
            let file = file.as_ref();
            let text = fs::read_to_string(file).with_context(|| format!("cannot read {}", file.display()))?;
            let typst = file.extension().is_some_and(|e| e == "typ");
            cited.extend(cited_keys(&text, typst).into_iter().filter(|k| !cited.contains(k)).collect::<Vec<_>>());
        }
        let library: Vec<String> = self.papers().0.into_iter().map(|p| p.id).collect();
        let before = self.project(name).unwrap_or_default();
        let (known, missing): (Vec<String>, Vec<String>) = cited.iter().cloned().partition(|k| library.contains(k));
        self.add_to_project(name, &known)?;
        let added = known.into_iter().filter(|k| !before.contains(k)).collect();
        let uncited = self.project(name)?.into_iter().filter(|k| !cited.contains(k)).collect();
        Ok((added, missing, uncited))
    }

    /// The projects a paper is in.
    pub fn projects_of(&self, id: &str) -> Vec<String> {
        let mut names = self.projects();
        names.retain(|name| self.project(name).is_ok_and(|ids| ids.iter().any(|i| i == id)));
        names
    }

    /// Follow a renamed paper (`Some(new id)`) or forget a removed one (`None`) in every project.
    pub(crate) fn replace_in_projects(&self, id: &str, new: Option<&str>) -> Result<()> {
        for name in self.projects() {
            let path = self.project_path(&name)?;
            let text = fs::read_to_string(&path)?;
            if !text.lines().any(|l| line_id(l) == id) {
                continue;
            }
            let lines = text.lines().filter_map(|l| match line_id(l) == id {
                true => new.map(|new| l.replacen(id, new, 1)),
                false => Some(l.to_string()),
            });
            fs::write(&path, lines.collect::<Vec<_>>().join("\n") + "\n")?;
        }
        Ok(())
    }
}

/// Citation keys in LaTeX (`\cite{a,b}`, `\citep[p.~3]{a}`, `\parencite`, `\nocite`, ...) or
/// Typst (`@a`, `#cite(<a>)`) source, in order of first use.
pub fn cited_keys(text: &str, typst: bool) -> Vec<String> {
    // Commented-out citations don't count.
    let lines: Vec<&str> = text.lines().map(|line| uncommented(line, typst)).collect();
    let text = lines.join("\n");
    let text = text.as_str();
    let mut keys: Vec<String> = vec![];
    let mut push = |key: &str| {
        let key = key.trim();
        if !key.is_empty() && key != "*" && !keys.iter().any(|k| k == key) {
            keys.push(key.to_string());
        }
    };
    if typst {
        for (i, _) in text.match_indices('@') {
            // Not e-mail addresses, package imports ("@preview/...") or URLs.
            if text[..i].chars().next_back().is_some_and(|c| c.is_alphanumeric() || "\"/".contains(c)) {
                continue;
            }
            let key: String =
                text[i + 1..].chars().take_while(|c| c.is_alphanumeric() || "_-:.".contains(*c)).collect();
            push(key.trim_end_matches(['.', ':']));
        }
        for (i, _) in text.match_indices("cite(<") {
            if let Some(end) = text[i + 6..].find('>') {
                push(&text[i + 6..i + 6 + end]);
            }
        }
    } else {
        for (i, _) in text.match_indices('\\') {
            let command: String = text[i + 1..].chars().take_while(char::is_ascii_alphabetic).collect();
            if !command.to_lowercase().contains("cite") {
                continue;
            }
            // Skip a star and optional [..] arguments, then read the {keys}.
            let mut rest = text[i + 1 + command.len()..].trim_start_matches('*').trim_start();
            while let Some(opt) = rest.strip_prefix('[') {
                rest = opt.split_once(']').map_or("", |(_, after)| after.trim_start());
            }
            if let Some((list, _)) = rest.strip_prefix('{').and_then(|r| r.split_once('}')) {
                list.split(',').for_each(&mut push);
            }
        }
    }
    keys
}

/// A line without its comment: LaTeX `%` (unless escaped as `\%`), Typst `//` (unless in a URL).
// ponytail: Typst /* block comments */ still count.
fn uncommented(line: &str, typst: bool) -> &str {
    let (marker, escape) = if typst { ("//", ':') } else { ("%", '\\') };
    let mut from = 0;
    while let Some(i) = line[from..].find(marker) {
        let at = from + i;
        if !line[..at].ends_with(escape) {
            return &line[..at];
        }
        from = at + marker.len();
    }
    line
}

fn line_id(line: &str) -> &str {
    line.split('#').next().unwrap_or("").trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn citations() {
        let tex = r"As \citet{vaswani2017attention} and \citep[see][p.~3]{ho2020denoising, smith:2020} show; \nocite{*} \cite*{x}";
        assert_eq!(cited_keys(tex, false), ["vaswani2017attention", "ho2020denoising", "smith:2020", "x"]);
        let typ = "#import \"@preview/cetz:0.2.2\"\nSee @vaswani2017attention. Mail me@uni.de, also #cite(<ho2020denoising>) and @fig:plot";
        assert_eq!(cited_keys(typ, true), ["vaswani2017attention", "fig:plot", "ho2020denoising"]);
        // Comments, escaped percent signs, URLs, keys over several lines, empty and odd commands.
        let tex = "100\\% sure \\cite{a} % \\cite{commented}\n% \\citep{gone}\n\\citeauthor*{b,\n c}\\cite{}\\excite\\cite[x]";
        assert_eq!(cited_keys(tex, false), ["a", "b", "c"]);
        let typ = "See https://example.org/@notakey and @a // @commented\n// @gone\n@b:";
        assert_eq!(cited_keys(typ, true), ["a", "b"]);
        assert!(cited_keys("", false).is_empty() && cited_keys("@", true).is_empty());
    }
}
