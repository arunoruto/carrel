//! JabRef's additions to a .bib: `file` links, `groups` memberships, and `@Comment{jabref-meta: ...}`
//! blocks naming the folder the links are relative to and the group tree. `carrel import` turns links
//! into the PDF and supplementary files and static groups into projects; `carrel bib --jabref` writes them.

use crate::bib::closing;
use crate::{Library, Paper, SUPPLEMENTARY};
use std::path::{Path, PathBuf};

/// What a .bib's `jabref-meta` comments say.
#[derive(Debug, Default, PartialEq)]
pub struct Meta {
    /// `fileDirectory` values, the user-specific ones first.
    pub dirs: Vec<String>,
    /// Static groups: their names, and the entry keys an old JabRef listed in the group itself.
    pub groups: Vec<(String, Vec<String>)>,
    /// How many groups aren't static (keyword, search, ... groups), which carrel can't keep.
    pub dynamic: usize,
}

pub fn meta(src: &str) -> Meta {
    let mut meta = Meta::default();
    for (at, _) in src.match_indices('@') {
        if !src[at + 1..].get(..7).is_some_and(|k| k.eq_ignore_ascii_case("comment")) {
            continue;
        }
        let rest = src[at + 8..].trim_start();
        let Some(end) = rest.starts_with('{').then(|| closing(rest)).flatten() else { continue };
        let body = rest[1..end].trim_start().strip_prefix("jabref-meta:");
        let Some((key, value)) = body.and_then(|b| b.split_once(':')) else { continue };
        let items = split(value, ';');
        let first = items.first().map(|d| d.trim().to_string()).filter(|d| !d.is_empty());
        match key.trim() {
            "fileDirectory" => meta.dirs.extend(first),
            k if k.starts_with("fileDirectory-") => {
                if let Some(dir) = first {
                    meta.dirs.insert(0, dir);
                }
            }
            "grouping" | "groupstree" => {
                for line in items.iter().map(|l| l.trim()).filter(|l| !l.is_empty()) {
                    // `<level> <type>:<name>;<context>;...`; carrel's projects are flat, so the level goes.
                    let group = line.split_once(' ').map_or(line, |(_, g)| g.trim_start());
                    if let Some(g) = group.strip_prefix("StaticGroup:") {
                        meta.groups.push((split(g, ';').swap_remove(0), vec![]));
                    } else if let Some(g) = group.strip_prefix("ExplicitGroup:") {
                        let mut tokens = split(g, ';').into_iter();
                        let name = tokens.next().unwrap_or_default();
                        meta.groups.push((name, tokens.skip(1).filter(|k| !k.is_empty()).collect()));
                    } else if !group.starts_with("AllEntriesGroup:") {
                        meta.dynamic += 1;
                    }
                }
            }
            _ => {}
        }
    }
    meta
}

/// `s` split at each `sep` not escaped with `\`, the escapes removed.
fn split(s: &str, sep: char) -> Vec<String> {
    let (mut parts, mut part, mut escaped) = (vec![], String::new(), false);
    for c in s.chars() {
        match c {
            _ if escaped => {
                part.push(c);
                escaped = false;
            }
            '\\' => escaped = true,
            _ if c == sep => parts.push(std::mem::take(&mut part)),
            _ => part.push(c),
        }
    }
    parts.push(part);
    parts
}

/// A field's value without its `{}` or `""`.
fn unwrap(raw: &str) -> &str {
    let braced = raw.strip_prefix('{').and_then(|r| r.strip_suffix('}'));
    braced.or_else(|| raw.strip_prefix('"').and_then(|r| r.strip_suffix('"'))).unwrap_or(raw)
}

/// The (path, type) pairs a raw `file` field links to: `description:path:type` links separated by `;`,
/// with `\` escaping. Like JabRef's FileFieldParser, a drive letter (`C:\x.pdf`) stays in the path.
pub fn files(raw: &str) -> Vec<(String, String)> {
    let value = unwrap(raw).replace("$\\backslash$", "\\");
    let chars: Vec<char> = value.chars().collect();
    let (mut links, mut parts, mut part) = (vec![], vec![], String::new());
    let (mut escaped, mut windows) = (false, false);
    for (i, &c) in chars.iter().enumerate() {
        match c {
            _ if escaped => {
                part.push(c);
                escaped = false;
            }
            '\\' if windows => part.push(c),
            '\\' => escaped = true,
            ':' if part.chars().count() == 1 && (parts.len() == 1 || chars.get(i + 1) == Some(&'\\')) => {
                part.push(c);
                windows = true;
            }
            ':' => parts.push(std::mem::take(&mut part)),
            ';' => {
                parts.push(std::mem::take(&mut part));
                links.push(std::mem::take(&mut parts));
                windows = false;
            }
            _ => part.push(c),
        }
    }
    parts.push(part);
    links.push(parts);
    let link = |parts: Vec<String>| {
        let get = |i: usize| parts.get(i).map_or("", |p| p.trim());
        let path = match (get(0), get(1), get(2)) {
            (_, path, _) if !path.is_empty() => path,
            ("", "", path) => path, // `::file.pdf`
            (path, "", "") => path, // a bare path, as Zotero writes
            _ => "",
        };
        (!path.is_empty()).then(|| (path.to_string(), get(2).to_string()))
    };
    links.into_iter().filter_map(link).collect()
}

/// Where a linked file is: an absolute path (Windows ones too) as it is, a relative one in the first of
/// `dirs` holding it.
pub fn resolve(link: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let drive = link.get(1..3).is_some_and(|s| s == ":\\" || s == ":/");
    match Path::new(link).is_absolute() || drive {
        true => Some(PathBuf::from(link)).filter(|p| p.is_file()),
        false => dirs.iter().map(|d| d.join(link)).find(|p| p.is_file()),
    }
}

/// The group names in a raw `groups` field. A `known` group whose name has commas spans several of the
/// comma-separated parts.
pub fn group_names(raw: &str, known: &[String]) -> Vec<String> {
    let parts: Vec<&str> = unwrap(raw).split(',').map(str::trim).collect();
    let (mut names, mut i) = (vec![], 0);
    while i < parts.len() {
        let spanning =
            |n: usize| known.iter().find(|k| k.split(',').map(str::trim).eq(parts[i..i + n].iter().copied()));
        let longest = (1..=parts.len() - i).rev().find_map(|n| spanning(n).map(|k| (n, k.clone())));
        let (n, name) = longest.unwrap_or((1, parts[i].to_string()));
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
        i += n;
    }
    names
}

/// A path in a `file` link, escaped.
fn link(path: &str, kind: &str) -> String {
    format!(":{}:{kind}", path.replace('\\', "\\\\").replace(':', "\\:").replace(';', "\\;"))
}

/// Fields only biblatex knows: their presence makes JabRef open the file in biblatex mode.
const BIBLATEX: [&str; 5] = ["journaltitle", "date", "eprinttype", "eprintclass", "urldate"];

impl Library {
    /// `papers` as a .bib for JabRef: each entry links its PDF and supplementary files, relative to the
    /// library (the `fileDirectory`), and lists its projects in `groups`; every project is a static group.
    pub fn jabref_bib(&self, papers: &[Paper]) -> String {
        let projects: Vec<(String, Vec<String>)> =
            self.projects().into_iter().map(|name| (name.clone(), self.project(&name).unwrap_or_default())).collect();
        let mut out = String::new();
        for p in papers {
            let mut bib = p.bib.clone();
            let mut links: Vec<String> =
                p.pdf().map(|_| link(&format!("{0}/{0}.pdf", p.id), "PDF")).into_iter().collect();
            let names = p.supplementary();
            for name in &names {
                // Text carrel extracted from a supplementary PDF isn't the author's.
                let extracted = name.strip_suffix(".md").is_some_and(|stem| names.contains(&format!("{stem}.pdf")));
                if extracted || p.dir.join(SUPPLEMENTARY).join(name).is_dir() {
                    continue;
                }
                let kind = if name.to_lowercase().ends_with(".pdf") { "PDF" } else { "" };
                links.push(link(&format!("{}/{SUPPLEMENTARY}/{name}", p.id), kind));
            }
            bib.set("file", Some(&links.join(";")));
            let groups: Vec<&str> =
                projects.iter().filter(|(_, ids)| ids.contains(&p.id)).map(|(name, _)| name.as_str()).collect();
            bib.set("groups", None);
            if !groups.is_empty() {
                bib.set_raw("groups", &format!("{{{}}}", groups.join(", ")));
            }
            out += &bib.to_bib(&p.id);
            out.push('\n');
        }
        if papers.iter().any(|p| BIBLATEX.iter().any(|f| p.bib.raw(f).is_some())) {
            out += "@Comment{jabref-meta: databaseType:biblatex;}\n\n";
        }
        let root = self.root.display().to_string().replace('\\', "\\\\").replace(';', "\\;");
        out += &format!("@Comment{{jabref-meta: fileDirectory:{root};}}\n");
        // Project names are ids, so they need no escaping here (no `;` or `\`).
        if !projects.is_empty() {
            out += "\n@Comment{jabref-meta: grouping:\n0 AllEntriesGroup:;\n";
            for (name, _) in &projects {
                out += &format!("1 StaticGroup:{name}\\;0\\;1\\;\\;\\;\\;;\n");
            }
            out += "}\n";
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files_of(raw: &str) -> Vec<String> {
        files(raw).into_iter().map(|(path, _)| path).collect()
    }

    #[test]
    fn file_fields() {
        assert_eq!(
            files(r"{Main:a/b.pdf:PDF;:other\:one\;two.pdf:PDF;Data:x.csv:}"),
            [("a/b.pdf".into(), "PDF".into()), ("other:one;two.pdf".into(), "PDF".into()), ("x.csv".into(), "".into())]
        );
        assert_eq!(files_of(r"{:C:\papers\x.pdf:PDF}"), [r"C:\papers\x.pdf"], "a drive letter");
        assert_eq!(files_of(r"{:C\:\\papers\\x.pdf:PDF}"), [r"C:\papers\x.pdf"], "the same, escaped");
        assert_eq!(files_of(r"{C:\papers\x.pdf}"), [r"C:\papers\x.pdf"], "Zotero's bare path");
        assert_eq!(files_of("{/home/me/Über café.pdf}"), ["/home/me/Über café.pdf"]);
        assert_eq!(files_of("\"::only-type.pdf\""), ["only-type.pdf"]);
        assert!(files("{}").is_empty() && files("{;;}").is_empty() && files("{Desc::PDF;}").is_empty());
        assert_eq!(files_of(r"{:x.pdf:PDF;:C:}"), ["x.pdf", "C:"], "no panic at the end");
    }

    #[test]
    fn metadata() {
        let src = r"@Comment{jabref-meta: databaseType:biblatex;}
@comment{jabref-meta: fileDirectory:papers;}
@Comment{jabref-meta: fileDirectory-me-laptop:/home/me/pdfs;}
@Comment{jabref-meta: grouping:
0 AllEntriesGroup:;
1 StaticGroup:Chapter 1\;0\;1\;0x8a8a8aff\;\;\;;
2 StaticGroup:Methods, old\;0\;1\;\;\;\;;
2 StaticGroup:a\\\;b\;0\;1\;\;\;\;;
1 KeywordGroup:ml\;0\;keywords\;ml\;0\;0\;1\;\;\;\;;
1 ExplicitGroup:Legacy\;0\;key1\;key2\;;
1 StaticGroup:Chapter 1\;0\;1\;\;\;\;;
}
@article{a, title = {A}}";
        let meta = meta(src);
        assert_eq!(meta.dirs, ["/home/me/pdfs", "papers"]);
        let names: Vec<&str> = meta.groups.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["Chapter 1", "Methods, old", "a;b", "Legacy", "Chapter 1"]);
        assert_eq!(meta.groups[3].1, ["key1", "key2"]);
        assert_eq!(meta.dynamic, 1);
        assert_eq!(super::meta("@article{a, title = {no metadata}}"), Meta::default());

        let known: Vec<String> = names.iter().map(|n| n.to_string()).collect();
        assert_eq!(
            group_names("{Chapter 1,Methods, old , a;b, new}", &known),
            ["Chapter 1", "Methods, old", "a;b", "new"]
        );
        assert_eq!(group_names("{Methods}", &known), ["Methods"]);
        assert!(group_names("{}", &known).is_empty() && group_names("{ , }", &[]).is_empty());
    }
}
