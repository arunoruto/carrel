//! A paper's metadata is one BibTeX entry in `<key>.bib`. Values are kept exactly as written, so
//! changing one field never reformats the others, and the files can be concatenated into a bibliography.

use anyhow::{Context, Result};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Entry {
    /// article, inproceedings, misc, ...
    pub kind: String,
    /// Lowercase field names with raw values, delimiters included: `{...}`, `"..."` or a bare number/macro.
    pub fields: Vec<(String, String)>,
}

/// Identifier fields, which BibTeX reads verbatim, so they must not be escaped.
const VERBATIM: [&str; 4] = ["doi", "url", "eprint", "file"];

impl Entry {
    pub fn new(kind: &str) -> Self {
        Self { kind: kind.into(), fields: vec![] }
    }

    /// Parse a file holding one entry. Returns its key too.
    pub fn parse(src: &str) -> Result<(String, Self)> {
        let start = src.find('@').context("no @entry found")?;
        Self::parse_at(&src[start..]).map(|(key, entry, _)| (key, entry))
    }

    /// Every entry of a .bib file with many, such as a LaTeX project's or a Zotero export.
    /// `@string`, `@preamble` and `@comment` blocks and stray `@`s (in e-mail addresses, say) are skipped.
    // ponytail: @string macros aren't expanded and # concatenation isn't supported; rare in exports.
    pub fn parse_all(src: &str) -> Vec<Result<(String, Self)>> {
        let mut entries = vec![];
        let mut rest = src;
        while let Some(at) = rest.find('@') {
            rest = &rest[at..];
            let kind = rest[1..].split(['{', '(']).next().unwrap_or("").trim();
            if kind.is_empty() || !kind.chars().all(|c| c.is_ascii_alphabetic()) {
                rest = &rest[1..];
                continue;
            }
            if ["string", "preamble", "comment"].contains(&kind.to_lowercase().as_str()) {
                let open = &rest[1 + kind.len()..].trim_start();
                match closing(open) {
                    Some(end) => rest = &open[end + 1..],
                    None => break,
                }
                continue;
            }
            match Self::parse_at(rest) {
                Ok((key, entry, len)) => {
                    entries.push(Ok((key, entry)));
                    rest = &rest[len..];
                }
                Err(e) => {
                    entries.push(Err(e));
                    rest = &rest[1..];
                }
            }
        }
        entries
    }

    /// Replace bare `@string` macro names in the field values by their text, so the entry stands on its
    /// own (month macros like `jun` are built into BibTeX and stay).
    pub fn expand(&mut self, macros: &[(String, String)]) {
        for (_, raw) in &mut self.fields {
            if let Some((_, text)) = macros.iter().find(|(name, _)| name.eq_ignore_ascii_case(raw)) {
                *raw = text.clone();
            }
        }
    }

    /// The `@string{name = "text"}` macros of a .bib file, with their raw values.
    pub fn macros(src: &str) -> Vec<(String, String)> {
        let lower = src.to_ascii_lowercase(); // same length, so its offsets slice `src`
        let found = lower.match_indices("@string").filter_map(|(i, _)| {
            let body = src[i + 7..].trim_start().strip_prefix(['{', '('])?;
            let (name, value) = body.split_once('=')?;
            let value = value.trim_start();
            let end = closing(value)?;
            Some((name.trim().to_string(), value[..=end].to_string()))
        });
        found.collect()
    }

    /// Parse the entry `src` starts with (at its `@`); also returns how many bytes it took.
    fn parse_at(src: &str) -> Result<(String, Self, usize)> {
        let rest = &src[1..];
        let open = rest.find(['{', '(']).context("no { after the entry type")?;
        let kind = rest[..open].trim().to_lowercase();
        let body = &rest[open + 1..];
        let comma = body.find(',').context("no key")?;
        let key = body[..comma].trim().to_string();
        let mut s = &body[comma + 1..];
        let mut fields = vec![];
        loop {
            s = s.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
            if s.is_empty() || s.starts_with(['}', ')']) {
                break;
            }
            let eq = s.find('=').context("a field without =")?;
            let name = s[..eq].trim().to_lowercase();
            s = s[eq + 1..].trim_start();
            let end = match s.chars().next() {
                Some('{' | '"') => closing(s).with_context(|| format!("unbalanced braces in {name}"))? + 1,
                _ => s.find([',', '}', ')', '\n']).unwrap_or(s.len()),
            };
            fields.push((name, s[..end].trim().to_string()));
            s = &s[end..];
        }
        let used = src.len() - s.len() + usize::from(!s.is_empty());
        Ok((key, Self { kind, fields }, used))
    }

    pub fn to_bib(&self, key: &str) -> String {
        let width = self.fields.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
        let fields: String = self.fields.iter().map(|(name, raw)| format!("  {name:<width$} = {raw},\n")).collect();
        format!("@{}{{{key},\n{fields}}}\n", self.kind)
    }

    /// A field's value exactly as written, delimiters included.
    pub fn raw(&self, name: &str) -> Option<&str> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, raw)| raw.as_str())
    }

    /// Set a field to a value as it should be written, delimiters included.
    pub fn set_raw(&mut self, name: &str, raw: &str) {
        match self.fields.iter().position(|(n, _)| n == name) {
            Some(i) => self.fields[i].1 = raw.to_string(),
            None => self.fields.push((name.to_string(), raw.to_string())),
        }
    }

    /// A field as plain text: braces, quotes and LaTeX escapes removed.
    pub fn get(&self, name: &str) -> Option<String> {
        let raw = self.raw(name)?;
        let inner = raw.strip_prefix('"').and_then(|r| r.strip_suffix('"')).unwrap_or(raw);
        Some(plain(inner)).filter(|s| !s.is_empty())
    }

    /// Set a field from plain text, escaped for BibTeX. `None` or "" removes it.
    pub fn set(&mut self, name: &str, value: Option<&str>) {
        let raw = value.map(str::trim).filter(|v| !v.is_empty()).map(|v| match name {
            _ if VERBATIM.contains(&name) => format!("{{{v}}}"),
            // Double braces keep the title's capitalization in every bibliography style.
            "title" => format!("{{{{{}}}}}", escape(v)),
            _ => format!("{{{}}}", escape(v)),
        });
        match raw {
            Some(raw) => self.set_raw(name, &raw),
            None => self.fields.retain(|(n, _)| n != name),
        }
    }

    pub fn title(&self) -> String {
        self.get("title").unwrap_or_default()
    }

    /// As written: usually "Family, Given".
    pub fn authors(&self) -> Vec<String> {
        let all = self.get("author").unwrap_or_default();
        all.split(" and ").map(str::trim).filter(|a| !a.is_empty()).map(String::from).collect()
    }

    pub fn year(&self) -> Option<i32> {
        self.get("year").or_else(|| self.get("date"))?.get(..4)?.parse().ok()
    }

    pub fn venue(&self) -> Option<String> {
        ["journal", "journaltitle", "booktitle", "publisher", "archiveprefix"].iter().find_map(|f| self.get(f))
    }

    pub fn doi(&self) -> Option<String> {
        self.get("doi")
    }

    /// The arXiv id in `eprint` (BibTeX `archiveprefix` or biblatex `eprinttype` name the archive).
    pub fn arxiv(&self) -> Option<String> {
        let archive = self.get("archiveprefix").or_else(|| self.get("eprinttype")).unwrap_or("arxiv".into());
        self.get("eprint").filter(|_| archive.eq_ignore_ascii_case("arxiv"))
    }

    /// The NASA ADS bibcode, kept in `adsurl` as ADS's own BibTeX has it.
    pub fn bibcode(&self) -> Option<String> {
        let url = self.get("adsurl")?;
        let (_, rest) = url.rsplit_once("/abs/")?;
        let bibcode = rest.split(['/', '?', '#']).next()?.replace("%26", "&");
        crate::fetch::is_bibcode(&bibcode).then_some(bibcode)
    }

    /// Keys this entry also goes by (biblatex's `ids`), such as its keys before a rename or merge.
    pub fn aliases(&self) -> Vec<String> {
        let all = self.get("ids").unwrap_or_default();
        let mut aliases: Vec<String> =
            all.split(',').map(str::trim).filter(|a| !a.is_empty()).map(String::from).collect();
        aliases.dedup();
        aliases
    }

    pub fn keywords(&self) -> Vec<String> {
        let all = self.get("keywords").unwrap_or_default();
        all.split([',', ';']).map(str::trim).filter(|k| !k.is_empty()).map(String::from).collect()
    }
}

/// The family name from "Family, Given" or "Given Family".
pub fn family(author: &str) -> &str {
    match author.split_once(',') {
        Some((family, _)) => family.trim(),
        None => author.split_whitespace().last().unwrap_or(author),
    }
}

/// Byte index of the delimiter closing the `{` or `"` that starts `s`.
pub(crate) fn closing(s: &str) -> Option<usize> {
    let quoted = s.starts_with('"');
    let (mut depth, mut escaped) = (0, false);
    for (i, c) in s.char_indices().skip(1) {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '{' => depth += 1,
            '}' if depth == 0 => return (!quoted).then_some(i),
            '}' => depth -= 1,
            '"' if quoted && depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// Plain text as a BibTeX value. Math (`$...$`, which arXiv titles carry) and balanced braces pass
/// through so LaTeX keeps working; elsewhere the characters LaTeX treats specially are escaped.
fn escape(text: &str) -> String {
    let unescaped = |c: char| text.char_indices().filter(move |&(i, x)| x == c && !text[..i].ends_with('\\')).count();
    let math = unescaped('$') % 2 == 0;
    let braces = balanced(text);
    let (mut out, mut in_math, mut prev) = (String::new(), false, ' ');
    for c in text.chars() {
        if c == '$' && math && prev != '\\' {
            in_math = !in_math;
        } else if !in_math && prev != '\\' && ("&%#_$".contains(c) || !braces && "{}".contains(c)) {
            out.push('\\');
        }
        out.push(c);
        prev = c;
    }
    out
}

fn balanced(text: &str) -> bool {
    let (mut depth, mut escaped) = (0, false);
    for c in text.chars() {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '{' => depth += 1,
            '}' if depth == 0 => return false,
            '}' => depth -= 1,
            _ => {}
        }
    }
    depth == 0
}

/// Commands kept as words in plain text, so `$\beta$-VAE` reads (and searches) as "beta-VAE".
const SYMBOLS: [&str; 41] = [
    "alpha",
    "beta",
    "gamma",
    "delta",
    "epsilon",
    "varepsilon",
    "zeta",
    "eta",
    "theta",
    "vartheta",
    "iota",
    "kappa",
    "lambda",
    "mu",
    "nu",
    "xi",
    "pi",
    "rho",
    "sigma",
    "tau",
    "upsilon",
    "phi",
    "varphi",
    "chi",
    "psi",
    "omega",
    "Gamma",
    "Delta",
    "Theta",
    "Lambda",
    "Xi",
    "Pi",
    "Sigma",
    "Upsilon",
    "Phi",
    "Psi",
    "Omega",
    "ell",
    "infty",
    "LaTeX",
    "TeX",
];

/// Plain text from a raw value.
// ponytail: LaTeX accents (\"u) drop to the bare letter; carrel itself writes UTF-8.
fn plain(raw: &str) -> String {
    let mut out = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' | '}' | '$' => {}
            '~' => out.push(' '),
            '\\' => match chars.next() {
                Some(c) if c.is_ascii_alphabetic() => {
                    // A command such as \textit or \v: drop its name, keep its argument (or, for a
                    // symbol, its name).
                    let mut name = c.to_string();
                    while let Some(c) = chars.next_if(char::is_ascii_alphabetic) {
                        name.push(c);
                    }
                    if SYMBOLS.contains(&name.as_str()) {
                        out.push_str(&name);
                    }
                }
                Some('"' | '\'' | '`' | '^' | '~' | '=' | '.') | None => {}
                Some(c) => out.push(c),
            },
            _ => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_edit_write() {
        let src = "% from Google Scholar\n@Article{lecun2015deep,\n  title={Deep {L}earning \\& more},\n  author = \"LeCun, Yann and M{\\\"u}ller, J\\\"org\",\n  month = jun, year=2015,\n  doi = {10.1038/nature_14539}\n}\n";
        let (key, mut e) = Entry::parse(src).unwrap();
        assert_eq!((key.as_str(), e.kind.as_str()), ("lecun2015deep", "article"));
        assert_eq!(e.title(), "Deep Learning & more");
        assert_eq!(e.authors(), ["LeCun, Yann", "Muller, Jorg"]);
        assert_eq!((e.year(), e.get("month").as_deref()), (Some(2015), Some("jun")));
        e.set("keywords", Some("deep learning, CNNs_and_more"));
        e.set("month", None);
        let (_, again) = Entry::parse(&e.to_bib("lecun2015deep")).unwrap();
        assert_eq!(again, e);
        assert_eq!(again.keywords(), ["deep learning", "CNNs_and_more"]);
        assert_eq!(again.doi().as_deref(), Some("10.1038/nature_14539"));
        assert!(e.to_bib("k").contains("keywords = {deep learning, CNNs\\_and\\_more},"));
        assert_eq!((family("Vaswani, Ashish"), family("Ashish Vaswani")), ("Vaswani", "Vaswani"));
        assert!(Entry::parse("@misc{x, title = {open").is_err());
    }

    #[test]
    fn escaping() {
        let cases = [
            ("Deep learning & more", r"Deep learning \& more"),
            (
                r"Canonical expansions of $\mathbb R_{\mathcal Q}$-germs",
                r"Canonical expansions of $\mathbb R_{\mathcal Q}$-germs",
            ),
            ("Costs 5% of $3", r"Costs 5\% of \$3"),
            ("a {b c", r"a \{b c"),
            (r"already \& escaped", r"already \& escaped"),
            ("snake_case #1", r"snake\_case \#1"),
            ("The {RNA} world", "The {RNA} world"),
        ];
        for (text, raw) in cases {
            assert_eq!(escape(text), raw, "escaping {text:?}");
            let (_, e) = Entry::parse(&format!("@misc{{k, note = {{{raw}}}}}")).unwrap();
            assert!(e.get("note").is_some(), "{raw:?} must parse back");
        }
        let mut e = Entry::new("misc");
        e.set("title", Some(r"$\beta$-VAE: {Learning} & ~more_than 100%"));
        assert_eq!(e.title(), "beta-VAE: Learning & more_than 100%");
        e.set("title", Some("unbalanced } brace {"));
        assert_eq!(e.title(), "unbalanced } brace {");
        assert_eq!(
            plain(r#"Fig.~2 by Schr\"odinger, \textit{et al.}, $\mathcal{O}(n)$"#),
            "Fig. 2 by Schrodinger, et al., O(n)"
        );
    }

    #[test]
    fn odd_inputs() {
        // Parentheses as delimiters, CRLF line endings, a BOM, uppercase names, quoted values with braces
        // and escaped quotes, empty values, no trailing comma.
        let src = "\u{feff}@ARTICLE(key,\r\n  TITLE = \"A {\"}quoted\\\" title\",\r\n  note = {},\r\n  year = 2020)";
        let (key, e) = Entry::parse(src).unwrap();
        assert_eq!((key.as_str(), e.kind.as_str()), ("key", "article"));
        assert_eq!(e.year(), Some(2020));
        assert_eq!(e.get("note"), None);
        assert!(e.title().contains("quoted"));
        let (_, again) = Entry::parse(&e.to_bib("key")).unwrap();
        assert_eq!(again, e, "round trip");
        // Unicode keeps its byte boundaries; nested braces stay.
        let (_, e) = Entry::parse(
            "@misc{k, title = {Über {Größe} – 深度学习}, author = {Gödel, Kurt and {The ATLAS Collaboration}}}",
        )
        .unwrap();
        assert_eq!(e.title(), "Über Größe – 深度学习");
        assert_eq!(e.authors(), ["Gödel, Kurt", "The ATLAS Collaboration"]);
        // Field names without values, garbage between entries.
        assert!(Entry::parse("@misc{k, title}").is_err());
        assert_eq!(Entry::parse_all("junk @@ @misc{a, title={A}} more junk @ @misc{b,}").len(), 2);
        assert!(Entry::parse("no entry here").is_err());
    }

    #[test]
    fn many_entries() {
        let src = "Exported by me@example.org\n@string{nat = \"Nature\"}\n@comment{ignore {this}}\n\
                   @article{a, title = {One}, journal = nat}\n@inproceedings{b,\n  title = \"Two, too\",\n}\n@misc{c, title = {open";
        let entries = Entry::parse_all(src);
        let keys: Vec<_> = entries.iter().filter_map(|e| e.as_ref().ok()).map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["a", "b"]);
        assert_eq!(entries.len(), 3, "the broken last entry is reported");
        assert_eq!(entries[1].as_ref().unwrap().1.title(), "Two, too");
        let mut a = entries[0].as_ref().unwrap().1.clone();
        a.expand(&Entry::macros(src));
        assert_eq!(a.get("journal").as_deref(), Some("Nature"));
        // "İ" grows when lowercased: offsets into the lowercased text would miss or split characters.
        let macros = Entry::macros("% İİİ Straße\n@STRING{ist = {İstanbul}}");
        assert_eq!(macros, [("ist".to_string(), "{İstanbul}".to_string())]);
    }
}
