//! Turning a PDF into `fulltext.md` and figures in `figures/`. Each way of doing it is an [`Extractor`];
//! [`choose`] picks the one the config asks for, by default the best one installed.

use anyhow::{Context, Result, bail};
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
/// `docling`, `pymupdf` or a shell command. `docling` is where the config says Docling is.
pub fn choose(setting: &str, docling: Option<PathBuf>) -> Result<Option<Box<dyn Extractor>>> {
    let docling = docling.or_else(|| on_path("docling"));
    let pymupdf = option_env!("CARREL_DEFAULT_EXTRACTOR").is_some() || on_path("uv").is_some();
    pick(setting, docling, pymupdf)
}

fn pick(setting: &str, docling: Option<PathBuf>, pymupdf: bool) -> Result<Option<Box<dyn Extractor>>> {
    let docling = docling.map(|path| Box::new(Docling(path)) as Box<dyn Extractor>);
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
        let text = fs::read_to_string(&md)?.replace(&format!("]({artifacts}/"), "](figures/");
        fs::write(out.join("fulltext.md"), text)?;
        if work.join(&artifacts).is_dir() {
            fs::rename(work.join(&artifacts), out.join("figures"))?;
        }
        fs::remove_dir_all(&work)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picked(setting: &str, docling: bool, pymupdf: bool) -> Option<String> {
        let docling = docling.then(|| PathBuf::from("/opt/docling"));
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
        assert!(pick("pymupdf", None, false).is_err_and(|e| e.to_string().contains("uv")));
    }

    #[test]
    fn docling_output() {
        let tmp = std::env::temp_dir().join(format!("carrel-docling-{}", std::process::id()));
        let out = tmp.join("out");
        fs::create_dir_all(&out).unwrap();
        // What docling writes for `My Paper.pdf`, links relative to the .md.
        let fake = tmp.join("docling");
        fs::write(
            &fake,
            "#!/bin/sh\nmkdir -p 'docling/My Paper_artifacts'\nprintf png > 'docling/My Paper_artifacts/image_000000_ab12.png'\n\
             printf '# Title\\n\\n![Image](My Paper_artifacts/image_000000_ab12.png)\\n' > 'docling/My Paper.md'\n",
        )
        .unwrap();
        std::process::Command::new("chmod").arg("+x").arg(&fake).status().unwrap();

        Docling(fake.clone()).extract(&tmp.join("My Paper.pdf"), &out).unwrap();
        let text = fs::read_to_string(out.join("fulltext.md")).unwrap();
        assert_eq!(text, "# Title\n\n![Image](figures/image_000000_ab12.png)\n");
        assert!(out.join("figures/image_000000_ab12.png").exists());
        assert!(!out.join("docling").exists(), "its folder is cleaned up");

        fs::write(&fake, "#!/bin/sh\necho 'ConversionError: broken PDF' >&2\n").unwrap();
        let e = Docling(fake).extract(&tmp.join("x.pdf"), &out).unwrap_err();
        assert!(e.to_string().contains("broken PDF"), "{e}");
        fs::remove_dir_all(tmp).unwrap();
    }
}
