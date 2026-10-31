//! Turning a PDF into `fulltext.md` and figures in `figures/`. Each way of doing it is an [`Extractor`];
//! [`choose`] picks the one the config asks for, by default the best one installed.

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub trait Extractor {
    /// For messages: `docling`, `pymupdf` or the shell command.
    fn name(&self) -> String;
    /// Write `fulltext.md`, and figure images into `figures/`, into the folder `out`.
    fn extract(&self, pdf: &Path, out: &Path) -> Result<()>;
}

/// The extractor `setting` (the config's `extractor`) asks for: `auto` (Docling, else PyMuPDF, else none),
/// `docling`, `pymupdf` or a shell command. `docling` is where the config says Docling is: a program, or
/// the URL of a docling-serve; `formulas`, whether Docling decodes formulas.
pub fn choose(setting: &str, docling: Option<&str>, formulas: bool) -> Result<Option<Box<dyn Extractor>>> {
    let docling: Option<Box<dyn Extractor>> = match docling {
        Some(url) if url.starts_with("http://") || url.starts_with("https://") => {
            Some(Box::new(DoclingServe(url.trim_end_matches('/').into(), formulas)))
        }
        Some(path) => Some(Box::new(Docling(crate::expand_home(path), formulas))),
        None => on_path("docling").map(|path| Box::new(Docling(path, formulas)) as _),
    };
    let pymupdf = option_env!("CARREL_DEFAULT_EXTRACTOR").is_some() || on_path("uv").is_some();
    pick(setting, docling, pymupdf)
}

fn pick(setting: &str, docling: Option<Box<dyn Extractor>>, pymupdf: bool) -> Result<Option<Box<dyn Extractor>>> {
    let pymupdf = pymupdf.then(|| Box::new(PyMuPdf) as Box<dyn Extractor>);
    Ok(match setting.trim() {
        // Docling gives up on some PDFs that PyMuPDF reads fine.
        "auto" => match (docling, pymupdf) {
            (Some(docling), Some(pymupdf)) => Some(Box::new(Fallback(docling, pymupdf))),
            (docling, pymupdf) => docling.or(pymupdf),
        },
        "docling" => Some(docling.context("the extractor is docling, but there is no `docling` on PATH; install it or set `docling` in the config to where it is")?),
        "pymupdf" => Some(pymupdf.context("the extractor is pymupdf, which needs uv: https://docs.astral.sh/uv/")?),
        cmd => Some(Box::new(Shell(cmd.into()))),
    })
}

fn on_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).map(|dir| dir.join(name)).find(|path| path.is_file())
}

/// A shell command that reads `$PDF` and writes `fulltext.md` (and optionally `figures/`) into the
/// current directory.
pub struct Shell(pub String);

const EXTRACT_PY: &str = include_str!("extract.py");

impl Extractor for Shell {
    fn name(&self) -> String {
        self.0.clone()
    }

    fn extract(&self, pdf: &Path, out: &Path) -> Result<()> {
        let output = Command::new("sh")
            .args(["-c", &self.0])
            .current_dir(out)
            .env("PDF", pdf)
            .env("CARREL_EXTRACT_PY", EXTRACT_PY)
            .stdin(Stdio::null())
            .output()
            .context("cannot run the extractor")?;
        if !output.status.success() || !out.join("fulltext.md").exists() {
            bail!("text extraction failed: {}", String::from_utf8_lossy(&output.stderr).trim());
        }
        Ok(())
    }
}

/// pymupdf4llm: fast, fine for most papers. Runs `extract.py` through uv, or through the Python that
/// packagers bake in at build time with `CARREL_DEFAULT_EXTRACTOR` (the Nix package and the Docker image do).
pub struct PyMuPdf;

const PYMUPDF: &str = match option_env!("CARREL_DEFAULT_EXTRACTOR") {
    Some(cmd) => cmd,
    None => r#"uv run --quiet --no-project --with pymupdf4llm==1.28.2 python -I -c "$CARREL_EXTRACT_PY" "$PDF""#,
};

impl Extractor for PyMuPdf {
    fn name(&self) -> String {
        "pymupdf".into()
    }

    fn extract(&self, pdf: &Path, out: &Path) -> Result<()> {
        Shell(PYMUPDF.into()).extract(pdf, out)
    }
}

/// [Docling](https://github.com/docling-project/docling)'s command line: better tables and reading order,
/// but slow on a CPU, and it downloads its models on first use.
/// The second field: decode formulas into LaTeX (its formula model; slow on a CPU).
pub struct Docling(pub PathBuf, pub bool);

impl Extractor for Docling {
    fn name(&self) -> String {
        if self.1 { "docling with formulas".into() } else { "docling".into() }
    }

    fn extract(&self, pdf: &Path, out: &Path) -> Result<()> {
        // Docling writes `<stem>.md` and its images to `<stem>_artifacts/`, linked relative to the .md
        // when the output folder is relative. No OCR: born-digital papers don't need it, and it's slow.
        // Its own PDF parser fails whole documents over one page it can't read ("operation for array
        // attempted on object of type null"); pdfium, its other one, reads those.
        let work = out.join("docling");
        let mut failed = String::new();
        let mut md = None;
        for backend in [&[][..], &["--pdf-backend", "pypdfium2"]] {
            let output = Command::new(&self.0)
                .args(["--to", "md", "--image-export-mode", "referenced", "--no-ocr", "--output", "docling"])
                .args(backend)
                .args(self.1.then_some("--enrich-formula"))
                .arg(pdf)
                .current_dir(out)
                .stdin(Stdio::null())
                .output()
                .with_context(|| format!("cannot run {}", self.0.display()))?;
            // It exits with 0 also when the conversion failed, so look at what it wrote.
            let mut entries = fs::read_dir(&work).into_iter().flatten().flatten().map(|e| e.path());
            md = entries.find(|p| p.extension().is_some_and(|e| e == "md"));
            if md.is_some() {
                break;
            }
            let stderr = String::from_utf8_lossy(&output.stderr);
            failed = stderr.trim().lines().last().unwrap_or("it wrote no Markdown").to_string();
            let _ = fs::remove_dir_all(&work);
        }
        let Some(md) = md else { bail!("docling failed: {failed}") };
        let artifacts = format!("{}_artifacts", md.file_stem().unwrap_or_default().to_string_lossy());
        // Older versions (2.69) put the images next to the .md; newer ones (2.118) one `docling/` deeper,
        // with the links saying so.
        let text = fs::read_to_string(&md)?.replace(&format!("](docling/{artifacts}/"), "](figures/");
        let text = text.replace(&format!("]({artifacts}/"), "](figures/").replace('\0', NOT_A_CHARACTER);
        fs::write(out.join("fulltext.md"), text)?;
        if let Some(images) =
            [work.join(&artifacts), work.join("docling").join(&artifacts)].into_iter().find(|d| d.is_dir())
        {
            fs::rename(images, out.join("figures"))?;
        }
        fs::remove_dir_all(&work)?;
        Ok(())
    }
}

/// The first extractor, else the second where the first fails. PDFs longer than [`LONG`] pages go
/// straight to the second: a textbook takes Docling hours and gigabytes of memory.
pub struct Fallback(pub Box<dyn Extractor>, pub Box<dyn Extractor>);

/// Pages beyond which a PDF is a book rather than a paper, for [`Fallback`].
pub const LONG: usize = 150;

impl Extractor for Fallback {
    fn name(&self) -> String {
        format!("{}, else {}", self.0.name(), self.1.name())
    }

    fn extract(&self, pdf: &Path, out: &Path) -> Result<()> {
        if pages(pdf).is_some_and(|n| n > LONG) {
            return self.1.extract(pdf, out);
        }
        let Err(first) = self.0.extract(pdf, out) else { return Ok(()) };
        self.1.extract(pdf, out).map_err(|second| anyhow::anyhow!("{first:#}; {second:#}"))
    }
}

/// A PDF's page count: the largest `/Count` of its page tree nodes (`/Type /Pages`), which sit in the
/// file as text, or in compressed object streams (`/ObjStm`) in newer PDFs. None when it can't tell.
// ponytail: a byte scan, not a PDF parser; a page tree in an encrypted or oddly filtered stream is missed
pub fn pages(pdf: &Path) -> Option<usize> {
    let bytes = fs::read(pdf).ok()?;
    let mut texts = vec![];
    let mut from = 0;
    while let Some(at) = find(&bytes[from..], b"stream").map(|i| from + i) {
        from = at + 6;
        let dict = &bytes[at.saturating_sub(300)..at];
        if bytes[..at].ends_with(b"end") || find(dict, b"/ObjStm").is_none() || find(dict, b"/FlateDecode").is_none() {
            continue;
        }
        let start = from + bytes[from..].iter().take(2).take_while(|b| matches!(b, b'\r' | b'\n')).count();
        // At most 16 MB from each: a small stream can inflate to gigabytes.
        let mut inflated = vec![];
        let stream = std::io::Read::take(flate2::read::ZlibDecoder::new(&bytes[start..]), 16 << 20);
        if std::io::Read::read_to_end(&mut { stream }, &mut inflated).is_ok() {
            texts.push(inflated);
        }
    }
    texts.push(bytes);
    let mut most = None;
    for text in &texts {
        let mut from = 0;
        while let Some(at) = find(&text[from..], b"/Pages").map(|i| from + i) {
            from = at + 6;
            // `/Type /Pages`, `/Type/Pages`; not `/Pages 3 0 R` in the catalogue.
            let before: String = String::from_utf8_lossy(&text[at.saturating_sub(12)..at]).split_whitespace().collect();
            if !before.ends_with("/Type") {
                continue;
            }
            // Its dictionary: from the `<<` before to the `>>` after.
            let start = text[..at].windows(2).rposition(|w| w == b"<<").unwrap_or(0);
            let (mut depth, mut end) = (1, at);
            while depth > 0 && end + 1 < text.len() {
                match &text[end..end + 2] {
                    b"<<" => (depth, end) = (depth + 1, end + 2),
                    b">>" => (depth, end) = (depth - 1, end + 2),
                    _ => end += 1,
                }
            }
            let window = String::from_utf8_lossy(&text[start..end]).into_owned();
            let counts = window.match_indices("/Count").filter_map(|(i, _)| {
                window[i + 6..].trim_start().split(|c: char| !c.is_ascii_digit()).next()?.parse::<usize>().ok()
            });
            most = most.max(counts.max());
        }
    }
    most
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// A [docling-serve](https://github.com/docling-project/docling-serve) at this URL, so the work happens on
/// another machine. Its API key, if it wants one, is the config's `tokens.docling` or `DOCLING_SERVE_API_KEY`.
/// The second field: decode formulas, as for [`Docling`].
pub struct DoclingServe(pub String, pub bool);

impl Extractor for DoclingServe {
    fn name(&self) -> String {
        format!("docling at {}{}", self.0, if self.1 { " with formulas" } else { "" })
    }

    fn extract(&self, pdf: &Path, out: &Path) -> Result<()> {
        let pdf = fs::read(pdf).with_context(|| format!("cannot read {}", pdf.display()))?;
        let body = json!({
            "options": {"to_formats": ["md"], "image_export_mode": "embedded", "do_ocr": false,
                "do_formula_enrichment": self.1},
            "sources": [{"kind": "file", "base64_string": STANDARD.encode(pdf), "filename": "paper.pdf"}],
        });
        // ponytail: no time limit, like the local one; a hung server hangs the add.
        let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).build().into();
        let mut request = agent.post(format!("{}/v1/convert/source", self.0));
        if let Some(key) = crate::token(&["DOCLING_SERVE_API_KEY"], |t| t.docling.as_ref())? {
            request = request.header("X-Api-Key", key);
        }
        let mut response = request.send_json(body).with_context(|| format!("cannot reach {}", self.0))?;
        let status = response.status();
        // Embedded figures make it big.
        let reply: Value = response.body_mut().with_config().limit(1 << 30).read_json().unwrap_or_default();
        if !status.is_success() {
            bail!("docling at {} answered {status}: {}", self.0, reply["detail"]);
        }
        let md = reply["document"]["md_content"].as_str().filter(|md| !md.is_empty());
        let Some(md) = md.filter(|_| matches!(reply["status"].as_str(), Some("success" | "partial_success"))) else {
            bail!("docling at {} failed: {}", self.0, reply["errors"]);
        };
        fs::write(out.join("fulltext.md"), unembed(&md.replace('\0', NOT_A_CHARACTER), out)?)?;
        Ok(())
    }
}

/// For a glyph a PDF doesn't map to text: Docling writes NUL, which makes tools take the file for binary;
/// PyMuPDF writes this.
const NOT_A_CHARACTER: &str = "\u{FFFD}";

/// Docling's embedded images, `](data:image/png;base64,…)`, as files in `figures/` linked from there.
fn unembed(md: &str, out: &Path) -> Result<String> {
    let (mut text, mut rest, mut n) = (String::new(), md, 0);
    while let Some(i) = rest.find("](data:image/") {
        let (head, tail) = rest.split_at(i + 2);
        text.push_str(head);
        let end = tail.find(')').unwrap_or(tail.len());
        let (uri, after) = tail.split_at(end);
        let image = uri.strip_prefix("data:image/").and_then(|u| u.split_once(";base64,"));
        match image.and_then(|(kind, data)| Some((kind, STANDARD.decode(data).ok()?))) {
            Some((kind, bytes)) => {
                // `png`, `jpeg`, `svg+xml`: only letters, so never a path.
                let ext: String = kind.chars().take_while(char::is_ascii_alphanumeric).collect();
                let name = format!("figures/image_{n:03}.{}", if ext.is_empty() { "png" } else { &ext });
                fs::create_dir_all(out.join("figures"))?;
                fs::write(out.join(&name), bytes)?;
                text.push_str(&name);
                n += 1;
            }
            None => text.push_str(uri),
        }
        rest = after;
    }
    text.push_str(rest);
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picked(setting: &str, docling: bool, pymupdf: bool) -> Option<String> {
        let docling = docling.then(|| Box::new(Docling("/opt/docling".into(), false)) as _);
        pick(setting, docling, pymupdf).unwrap().map(|x| x.name())
    }

    #[test]
    fn choosing() {
        assert_eq!(picked("auto", true, true).as_deref(), Some("docling, else pymupdf"), "docling first");
        assert_eq!(picked("auto", true, false).as_deref(), Some("docling"));
        assert_eq!(picked("auto", false, true).as_deref(), Some("pymupdf"));
        assert_eq!(picked("auto", false, false), None, "nothing installed");
        assert_eq!(picked("pymupdf", true, true).as_deref(), Some("pymupdf"));
        assert_eq!(picked("echo hi", true, true).as_deref(), Some("echo hi"), "anything else is a command");
        assert!(pick("docling", None, true).is_err_and(|e| e.to_string().contains("no `docling`")));
        let server = choose("auto", Some("https://docling.example.org/"), false).unwrap().unwrap();
        assert!(server.name().starts_with("docling at https://docling.example.org"), "a URL is a docling-serve");
        assert!(pick("pymupdf", None, false).is_err_and(|e| e.to_string().contains("uv")));
    }

    #[test]
    fn docling_output() {
        let tmp = std::env::temp_dir().join(format!("carrel-docling-{}", std::process::id()));
        let out = tmp.join("out");
        fs::create_dir_all(&out).unwrap();
        // What docling 2.118 writes for `My Paper.pdf` (tests/cli.rs has the older layout).
        let fake = tmp.join("docling");
        fs::write(
            &fake,
            "#!/bin/sh\nmkdir -p 'docling/docling/My Paper_artifacts'\nprintf png > 'docling/docling/My Paper_artifacts/image_000000_ab12.png'\n\
             printf '# Title\\000\\n\\n![Image](My Paper_artifacts/image_000000_ab12.png)\\n![Image](docling/My Paper_artifacts/image_000000_ab12.png)\\n' > 'docling/My Paper.md'\n",
        )
        .unwrap();
        std::process::Command::new("chmod").arg("+x").arg(&fake).status().unwrap();

        Docling(fake.clone(), false).extract(&tmp.join("My Paper.pdf"), &out).unwrap();
        let text = fs::read_to_string(out.join("fulltext.md")).unwrap();
        assert_eq!(
            text,
            "# Title\u{FFFD}\n\n![Image](figures/image_000000_ab12.png)\n![Image](figures/image_000000_ab12.png)\n"
        );
        assert!(out.join("figures/image_000000_ab12.png").exists());
        assert!(!out.join("docling").exists(), "its folder is cleaned up");

        // Its parser fails; pdfium's works.
        fs::write(
            &fake,
            "#!/bin/sh\nmkdir -p docling\ncase \"$*\" in *pypdfium2*) echo 'from pdfium' > docling/x.md ;; \
             *) echo 'ConversionError: broken PDF' >&2 ;; esac\n",
        )
        .unwrap();
        Docling(fake.clone(), false).extract(&tmp.join("x.pdf"), &out).unwrap();
        assert_eq!(fs::read_to_string(out.join("fulltext.md")).unwrap(), "from pdfium\n");
        fs::write(&fake, "#!/bin/sh\nmkdir -p docling\necho 'ConversionError: broken PDF' >&2\n").unwrap();
        let e = Docling(fake.clone(), false).extract(&tmp.join("x.pdf"), &out).unwrap_err();
        assert!(e.to_string().contains("broken PDF"), "{e}");
        // Formulas: its formula model.
        fs::write(&fake, "#!/bin/sh\nmkdir -p docling\necho \"$*\" > docling/x.md\n").unwrap();
        Docling(fake.clone(), true).extract(&tmp.join("x.pdf"), &out).unwrap();
        assert!(fs::read_to_string(out.join("fulltext.md")).unwrap().contains("--enrich-formula"));
        assert!(!out.join("docling").exists(), "cleaned up after failing too");

        // Where Docling fails, PyMuPDF (here a command) takes over.
        let fails = Shell("exit 1".into());
        let works = Shell("echo fallback > fulltext.md".into());
        Fallback(Box::new(fails), Box::new(works)).extract(&tmp.join("x.pdf"), &out).unwrap();
        assert_eq!(fs::read_to_string(out.join("fulltext.md")).unwrap(), "fallback\n");
        let e = Fallback(Box::new(Shell("exit 1".into())), Box::new(Shell("exit 2".into())));
        assert!(e.extract(&tmp.join("x.pdf"), &out).is_err());
        fs::remove_dir_all(tmp).unwrap();
    }

    #[test]
    fn page_counts() {
        use std::io::Write;
        let tmp = std::env::temp_dir().join(format!("carrel-pages-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        // A page tree as text, with an outline whose /Count is not the pages'.
        let plain = tmp.join("plain.pdf");
        let text = "%PDF-1.4\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
                    2 0 obj << /Type /Pages /Kids [3 0 R 4 0 R] /Count 212 >> endobj\n\
                    5 0 obj << /Type /Outlines /Count 900 >> endobj\n%%EOF\n";
        fs::write(&plain, text).unwrap();
        assert_eq!(pages(&plain), Some(212));
        // The same in a compressed object stream, as PDF 1.5 writes it.
        let mut z = flate2::write::ZlibEncoder::new(vec![], flate2::Compression::default());
        z.write_all(b"2 0 << /Type/Pages/Kids[3 0 R]/Count 12>><</Type/Page/Parent 2 0 R>>").unwrap();
        let mut packed =
            b"%PDF-1.5\n7 0 obj\n<< /Type /ObjStm /N 2 /First 8 /Filter /FlateDecode /Length 60 >>\nstream\r\n"
                .to_vec();
        packed.extend(z.finish().unwrap());
        packed.extend(b"\nendstream\nendobj\n%%EOF\n");
        fs::write(tmp.join("packed.pdf"), packed).unwrap();
        assert_eq!(pages(&tmp.join("packed.pdf")), Some(12));
        // Its /Count after a nested dictionary, and a line break inside `/Type /Pages`.
        let nested = "2 0 obj << /Type\n/Pages /Resources << /Font << /F1 9 0 R >> >> /Count 33 >> endobj\n";
        fs::write(tmp.join("nested.pdf"), nested).unwrap();
        assert_eq!(pages(&tmp.join("nested.pdf")), Some(33));
        fs::write(tmp.join("none.pdf"), "%PDF-1.4\n%%EOF\n").unwrap();
        assert_eq!(pages(&tmp.join("none.pdf")), None);

        // A book skips the first extractor.
        let out = tmp.join("out");
        fs::create_dir_all(&out).unwrap();
        let first = Shell("echo first > fulltext.md".into());
        let second = Shell("echo second > fulltext.md".into());
        Fallback(Box::new(first), Box::new(second)).extract(&plain, &out).unwrap();
        assert_eq!(fs::read_to_string(out.join("fulltext.md")).unwrap(), "second\n");
        fs::remove_dir_all(tmp).unwrap();
    }

    #[test]
    fn docling_serve() {
        let tmp = std::env::temp_dir().join(format!("carrel-docling-serve-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("paper.pdf"), "%PDF-1.4").unwrap();
        // "cG5n" is "png"; the second image isn't base64 and stays as it is.
        let reply = r##"{"status": "success", "errors": [], "document": {"md_content":
            "# Title\n\n![Image](data:image/png;base64,cG5n)\n![x](data:image/../x;base64,!)\n"}}"##;
        let (url, server) = crate::fetch::fake_server(vec![
            (200, reply),
            (200, r#"{"status": "failure", "errors": ["broken PDF"], "document": {}}"#),
        ]);
        DoclingServe(url.clone(), false).extract(&tmp.join("paper.pdf"), &tmp).unwrap();
        let text = fs::read_to_string(tmp.join("fulltext.md")).unwrap();
        assert_eq!(text, "# Title\n\n![Image](figures/image_000.png)\n![x](data:image/../x;base64,!)\n");
        assert_eq!(fs::read(tmp.join("figures/image_000.png")).unwrap(), b"png");
        let e = DoclingServe(url, false).extract(&tmp.join("paper.pdf"), &tmp).unwrap_err();
        assert!(e.to_string().contains("broken PDF"), "{e}");
        let requests = server.join().unwrap();
        assert!(
            requests[0].starts_with("POST /v1/convert/source")
                && requests[0].contains(r#""base64_string": "JVBERi0xLjQ=""#),
            "{}",
            requests[0]
        );
        fs::remove_dir_all(tmp).unwrap();
    }
}
