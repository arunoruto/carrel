//! Tests against the real services: arXiv, doi.org, Crossref, Semantic Scholar. They need internet
//! (and `uv` for the real extractor), so they are ignored by default, which keeps `cargo test` and
//! sandboxed builds such as Nix offline. Run them with `cargo test -- --ignored`.
//! Semantic Scholar rate-limits anonymous use; set S2_API_KEY if those tests fail with 429.

mod common;

use carrel::Library;
use carrel::fetch::{self, Ids};
use common::*;

/// The services rate-limit: one test at a time.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn one_at_a_time() -> std::sync::MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner())
}

/// A library using the real extractor.
fn real_library(tmp: &Tmp) -> Library {
    let mut lib = library(tmp);
    lib.extractor = carrel::DEFAULT_EXTRACTOR.into();
    lib
}

fn ids(doi: Option<&str>, arxiv: Option<&str>) -> Ids {
    Ids { doi: doi.map(String::from), arxiv: arxiv.map(String::from) }
}

#[test]
#[ignore = "needs internet"]
fn arxiv_paper_with_pdf_and_text() {
    let _turn = one_at_a_time();
    let tmp = Tmp::new("net-arxiv");
    let lib = real_library(&tmp);
    let (id, messages) = lib.add("https://arxiv.org/abs/1706.03762v7").unwrap();
    assert_eq!(id, "vaswani2017attention", "{messages:?}");
    let p = lib.get(&id).unwrap();
    assert_eq!((p.bib.arxiv().as_deref(), p.bib.year()), (Some("1706.03762"), Some(2017)));
    assert_eq!(p.bib.authors().len(), 8);
    assert!(p.pdf().is_some() && p.fulltext().unwrap().contains("Transformer"));
    assert!(p.figures().iter().any(|f| f.starts_with("page-")), "{:?}", p.figures());
    assert!(lib.add("arXiv:1706.03762").unwrap_err().to_string().contains("already in the library"));
}

#[test]
#[ignore = "needs internet"]
fn journal_metadata() {
    let _turn = one_at_a_time();
    let tmp = Tmp::new("net-doi");
    let lib = library(&tmp);
    let (id, _) = lib.add("https://doi.org/10.1038/nature14539").unwrap();
    let b = lib.get(&id).unwrap().bib;
    assert_eq!((id.as_str(), b.kind.as_str()), ("lecun2015deep", "article"));
    assert_eq!((b.get("journal").as_deref(), b.get("pages").as_deref()), (Some("Nature"), Some("436--444")));

    let aps = fetch::metadata(&ids(Some("10.1103/PhysRevLett.116.061102"), None)).unwrap();
    assert_eq!(aps.get("pages").as_deref(), Some("061102"), "APS article numbers");
}

#[test]
#[ignore = "needs internet"]
fn arxiv_specialities() {
    let _turn = one_at_a_time();
    let old = fetch::metadata(&ids(None, Some("hep-th/9711200"))).unwrap();
    assert!(old.title().contains("Large N"), "{}", old.title());
    assert_eq!(old.arxiv().as_deref(), Some("hep-th/9711200"));
    let latex = fetch::metadata(&ids(None, Some("2609.13189"))).unwrap();
    assert!(latex.to_bib("k").contains(r"$\mathbb R_{\mathcal Q}$"), "math stays LaTeX: {}", latex.to_bib("k"));
}

#[test]
#[ignore = "needs internet"]
fn finding_by_title() {
    let _turn = one_at_a_time();
    let found = fetch::by_title("Denoising Diffusion Probabilistic Models").unwrap();
    assert_eq!(found.arxiv.as_deref(), Some("2006.11239"));
    let error = fetch::by_title("diffusion models survey").unwrap_err().to_string();
    assert!(error.contains("no paper is titled exactly") && error.lines().count() > 2, "{error}");
    assert!(!fetch::search("attention transformer translation").unwrap().is_empty());
}

#[test]
#[ignore = "needs internet"]
fn preprint_becomes_published() {
    let _turn = one_at_a_time();
    let tmp = Tmp::new("net-update");
    let lib = library(&tmp); // the fake extractor: only metadata matters here
    let (id, _) = lib.add("1311.2524").unwrap();
    assert_eq!(lib.get(&id).unwrap().bib.kind, "misc");
    let changes = lib.update(&id, None).unwrap();
    let b = lib.get(&id).unwrap().bib;
    assert_eq!((b.kind.as_str(), b.doi().as_deref()), ("inproceedings", Some("10.1109/CVPR.2014.81")), "{changes:?}");
    assert_eq!(b.arxiv().as_deref(), Some("1311.2524"), "keeps its arXiv id");
    assert_eq!(lib.update(&id, None).unwrap(), ["already up to date"]);
}

#[test]
#[ignore = "needs internet"]
fn citation_graph() {
    let _turn = one_at_a_time();
    let tmp = Tmp::new("net-refs");
    let lib = library(&tmp);
    paper(&lib, "ddpm", "title = {Denoising Diffusion Probabilistic Models}, eprint = {2006.11239}");
    paper(&lib, "attention", "title = {Attention Is All You Need}, eprint = {1706.03762}");
    let references = lib.related("ddpm", false).unwrap();
    assert!(references.len() > 30);
    assert!(references.iter().any(|c| c.in_library.as_deref() == Some("attention")), "known papers are marked");
    assert!(!lib.related("attention", true).unwrap().is_empty(), "cited by others");
    assert!(
        fetch::published_doi("1512.03385").unwrap().is_some_and(|doi| doi.to_lowercase() == "10.1109/cvpr.2016.90")
    );
}

/// Each reference source on a lunar paper, by itself: they are interchangeable behind `ReferenceSource`.
#[test]
#[ignore = "needs internet"]
fn reference_sources() {
    use carrel::refs::{Ads, Crossref, Lookup, OpenAlex, ReferenceSource, SemanticScholar};
    let _turn = one_at_a_time();
    let paper = ids(Some("10.1029/2010JE003711"), None);
    let lookup = Lookup { ids: &paper, title: "" };
    let openalex = OpenAlex { api_key: std::env::var("OPENALEX_API_KEY").ok() }.references(&lookup).unwrap();
    assert!(openalex.len() > 80, "{}", openalex.len());
    assert!(openalex.iter().any(|c| c.ids.doi.is_some() && !c.authors.is_empty() && c.year.is_some()));
    let crossref = Crossref.references(&lookup).unwrap();
    assert!(crossref.len() > 80 && crossref.iter().any(|c| c.ids.doi.is_none()), "{}", crossref.len());
    // Wiley hides that paper's references from Semantic Scholar ("elided by the publisher"); arXiv's are open.
    match SemanticScholar.references(&Lookup { ids: &ids(None, Some("1706.03762")), title: "" }) {
        Ok(found) => assert!(found.len() > 25, "{}", found.len()),
        Err(e) => assert!(e.to_string().contains("rate-limiting"), "{e:#}"),
    }
    match std::env::var("ADS_API_TOKEN") {
        Ok(token) => assert!(Ads { token }.references(&lookup).unwrap().len() > 80),
        Err(_) => eprintln!("ADS skipped: no ADS_API_TOKEN"),
    }

    // Added by DOI, a paper comes with its references.
    let tmp = Tmp::new("net-refs");
    let lib = library(&tmp);
    let (id, messages) = lib.add("10.1126/science.1178105").unwrap();
    let refs = lib.references(&id, false).unwrap();
    assert!(refs.len() >= 15 && !refs[0].0.source.is_empty(), "{messages:?}");
    assert!(refs.iter().any(|(r, _)| r.source == "crossref"), "an LPSC abstract OpenAlex can't match");
}

/// Two libraries sharing through a real bucket: `CARREL_TEST_BUCKET` and the usual `AWS_ENDPOINT_URL`,
/// `AWS_REGION`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`. Skipped without them. Works in a folder of
/// its own in the bucket and empties it again.
#[test]
#[ignore = "needs internet"]
fn s3_remote() {
    let Ok(bucket) = std::env::var("CARREL_TEST_BUCKET") else { return eprintln!("no CARREL_TEST_BUCKET; skipped") };
    let tmp = Tmp::new("s3");
    let prefix = tmp.0.file_name().unwrap().to_string_lossy().into_owned();
    let config: carrel::remote::RemoteConfig =
        toml::from_str(&format!("type = \"s3\"\nbucket = \"{bucket}\"\nprefix = \"{prefix}\"\n")).unwrap();
    let remote = config.open().unwrap();
    let a = library(&tmp);
    let b = Library::open(tmp.path("libB")).unwrap();
    tmp.write("lib/a2020/a2020.bib", "@misc{a2020, title = {Ä paper}}\n");
    tmp.write("lib/a2020/figures/figure 1 – ü.png", [0u8, 1, 2, 255]);
    let sync = |lib: &Library| {
        let report = lib.sync(&*remote, None, false, |_, _| {}).unwrap();
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        report.done.len()
    };
    assert_eq!(sync(&a), 2);
    assert_eq!(sync(&a), 0, "S3's ETags match");
    assert_eq!(sync(&b), 2);
    assert_eq!(std::fs::read(b.root.join("a2020/figures/figure 1 – ü.png")).unwrap(), [0u8, 1, 2, 255]);
    std::fs::remove_dir_all(a.root.join("a2020")).unwrap();
    assert_eq!(sync(&a), 2);
    assert!(remote.list().unwrap().is_empty());
}
