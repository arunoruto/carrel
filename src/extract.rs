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
/// the URL of a docling-serve.
pub fn choose(setting: &str, docling: Option<&str>) -> Result<Option<Box<dyn Extractor>>> {
    let docling: Option<Box<dyn Extractor>> = match docling {
        Some(url) if url.starts_with("http://") || url.starts_with("https://") => {
            Some(Box::new(DoclingServe(url.trim_end_matches('/').into())))
        }
        Some(path) => Some(Box::new(Docling(crate::expand_home(path)))),
        None => on_path("docling").map(|path| Box::new(Docling(path)) as _),
    };
    let pymupdf = option_env!("CARREL_DEFAULT_EXTRACTOR").is_some() || on_path("uv").is_some();
    pick(setting, docling, pymupdf)
}

fn pick(setting: &str, docling: Option<Box<dyn Extractor>>, pymupdf: bool) -> Result<Option<Box<dyn Extractor>>> {
    let pymupdf = pymupdf.then(|| Box::new(PyMuPdf) as Box<dyn Extractor>);
    Ok(match setting.trim() {
        "auto" => docling.or(pymupdf),
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
pub struct Docling(pub PathBuf);

impl Extractor for Docling {
    fn name(&self) -> String {
        "docling".into()
    }

    fn extract(&self, pdf: &Path, out: &Path) -> Result<()> {
        // Docling writes `<stem>.md` and its images to `<stem>_artifacts/`, linked relative to the .md
        // when the output folder is relative. No OCR: born-digital papers don't need it, and it's slow.
        let output = Command::new(&self.0)
            .args(["--to", "md", "--image-export-mode", "referenced", "--no-ocr", "--output", "docling"])
            .arg(pdf)
            .current_dir(out)
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("cannot run {}", self.0.display()))?;
        let work = out.join("docling");
        let mut entries = fs::read_dir(&work).into_iter().flatten().flatten().map(|e| e.path());
        // It exits with 0 also when the conversion failed, so look at what it wrote.
        let Some(md) = entries.find(|p| p.extension().is_some_and(|e| e == "md")) else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("docling failed: {}", stderr.trim().lines().last().unwrap_or("it wrote no Markdown"));
        };
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

/// A [docling-serve](https://github.com/docling-project/docling-serve) at this URL, so the work happens on
/// another machine. Its API key, if it wants one, is the config's `tokens.docling` or `DOCLING_SERVE_API_KEY`.
pub struct DoclingServe(pub String);

impl Extractor for DoclingServe {
    fn name(&self) -> String {
        format!("docling at {}", self.0)
    }

    fn extract(&self, pdf: &Path, out: &Path) -> Result<()> {
        let pdf = fs::read(pdf).with_context(|| format!("cannot read {}", pdf.display()))?;
        let body = json!({
            "options": {"to_formats": ["md"], "image_export_mode": "embedded", "do_ocr": false},
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
        let docling = docling.then(|| Box::new(Docling("/opt/docling".into())) as _);
        pick(setting, docling, pymupdf).unwrap().map(|x| x.name())
    }

    #[test]
    fn choosing() {
        assert_eq!(picked("auto", true, true).as_deref(), Some("docling"), "docling first");
        assert_eq!(picked("auto", false, true).as_deref(), Some("pymupdf"));
        assert_eq!(picked("auto", false, false), None, "nothing installed");
        assert_eq!(picked("pymupdf", true, true).as_deref(), Some("pymupdf"));
        assert_eq!(picked("echo hi", true, true).as_deref(), Some("echo hi"), "anything else is a command");
        assert!(pick("docling", None, true).is_err_and(|e| e.to_string().contains("no `docling`")));
        let server = choose("auto", Some("https://docling.example.org/")).unwrap().unwrap();
        assert_eq!(server.name(), "docling at https://docling.example.org", "a URL is a docling-serve");
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

        Docling(fake.clone()).extract(&tmp.join("My Paper.pdf"), &out).unwrap();
        let text = fs::read_to_string(out.join("fulltext.md")).unwrap();
        assert_eq!(
            text,
            "# Title\u{FFFD}\n\n![Image](figures/image_000000_ab12.png)\n![Image](figures/image_000000_ab12.png)\n"
        );
        assert!(out.join("figures/image_000000_ab12.png").exists());
        assert!(!out.join("docling").exists(), "its folder is cleaned up");

        fs::write(&fake, "#!/bin/sh\necho 'ConversionError: broken PDF' >&2\n").unwrap();
        let e = Docling(fake).extract(&tmp.join("x.pdf"), &out).unwrap_err();
        assert!(e.to_string().contains("broken PDF"), "{e}");
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
        DoclingServe(url.clone()).extract(&tmp.join("paper.pdf"), &tmp).unwrap();
        let text = fs::read_to_string(tmp.join("fulltext.md")).unwrap();
        assert_eq!(text, "# Title\n\n![Image](figures/image_000.png)\n![x](data:image/../x;base64,!)\n");
        assert_eq!(fs::read(tmp.join("figures/image_000.png")).unwrap(), b"png");
        let e = DoclingServe(url).extract(&tmp.join("paper.pdf"), &tmp).unwrap_err();
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
