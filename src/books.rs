//! Books have an ISBN where papers have a DOI: printed on their copyright page, and known to Open Library,
//! which also finds one by title, author and year.

use crate::bib::{self, Entry};
use crate::fetch::{self, agent};
use anyhow::{Context, Result};
use serde_json::Value;

const OPEN_LIBRARY: &str = "https://openlibrary.org";

/// An ISBN-10 or ISBN-13 with or without hyphens and spaces, as ISBN-13 digits; None when its check
/// digit is wrong.
pub fn isbn13(raw: &str) -> Option<String> {
    let chars: String = raw.chars().filter(|c| !matches!(c, '-' | ' ' | '‐' | '–')).collect();
    let digit = |c: char| c.to_digit(10);
    match chars.len() {
        13 if chars.chars().all(|c| c.is_ascii_digit()) => {
            let sum: u32 =
                chars.chars().enumerate().map(|(i, c)| digit(c).unwrap() * if i % 2 == 0 { 1 } else { 3 }).sum();
            (sum % 10 == 0 && (chars.starts_with("978") || chars.starts_with("979"))).then_some(chars)
        }
        10 => {
            let (body, check) = chars.split_at(9);
            let body: Vec<u32> = body.chars().map(digit).collect::<Option<_>>()?;
            let check = match check {
                "X" | "x" => 10,
                c => digit(c.chars().next()?)?,
            };
            let sum: u32 = body.iter().enumerate().map(|(i, d)| d * (10 - i as u32)).sum::<u32>() + check;
            if sum % 11 != 0 {
                return None;
            }
            let twelve = format!("978{}", body.iter().map(u32::to_string).collect::<String>());
            let sum: u32 =
                twelve.chars().enumerate().map(|(i, c)| digit(c).unwrap() * if i % 2 == 0 { 1 } else { 3 }).sum();
            Some(format!("{twelve}{}", (10 - sum % 10) % 10))
        }
        _ => None,
    }
}

/// The first valid ISBN labelled as one ("ISBN 0-521-78252-X", "ISBN-13: 978-…") near the start of a
/// book's text, where its copyright page is.
pub fn find_isbn(text: &str) -> Option<String> {
    let head: String = text.chars().take(60_000).collect();
    let upper = head.to_uppercase();
    upper.match_indices("ISBN").find_map(|(i, _)| {
        // A label may say which kind: "ISBN-13:", "ISBN 10".
        let mut rest = &upper[i + 4..];
        for label in ["-13", "-10", " 13", " 10", "13", "10"] {
            if let Some(after) = rest.strip_prefix(label)
                && after.starts_with([':', ' ', '\u{a0}'])
            {
                rest = after;
                break;
            }
        }
        isbn_at(rest.trim_start_matches([':', ' ', '\u{a0}']))
    })
}

/// The ISBN at the start of `s`: digits (and a final X) with single hyphens or spaces between them.
fn isbn_at(s: &str) -> Option<String> {
    let mut end = 0;
    let mut digits = 0;
    for (i, c) in s.char_indices() {
        match c {
            '0'..='9' => digits += 1,
            'X' if digits == 9 => digits += 1,
            '-' | ' ' | '‐' | '–' if digits > 0 && digits < 13 => {}
            _ => break,
        }
        end = i + c.len_utf8();
        if digits == 13 {
            break;
        }
    }
    let candidate = s[..end].trim_end_matches(['-', ' ', '‐', '–']);
    // Thirteen digits read as ten would be a wrong ISBN-10, so try the longest first.
    isbn13(candidate).or_else(|| {
        let ten: String = candidate
            .chars()
            .scan(0, |n, c| {
                *n += c.is_ascii_alphanumeric() as usize;
                (*n <= 10).then_some(c)
            })
            .collect();
        isbn13(&ten)
    })
}

/// A book's metadata from Open Library: title, authors, publisher, year, pages and the ISBN.
pub fn by_isbn(isbn: &str) -> Result<Option<Entry>> {
    let url = format!("{OPEN_LIBRARY}/api/books?bibkeys=ISBN:{isbn}&format=json&jscmd=data");
    let v: Value = agent().get(&url).call().context("cannot reach Open Library")?.into_body().read_json()?;
    Ok(entry(&v[format!("ISBN:{isbn}")], isbn))
}

fn entry(book: &Value, isbn: &str) -> Option<Entry> {
    let title = book["title"].as_str()?;
    let title = match book["subtitle"].as_str() {
        Some(sub) => format!("{title}: {sub}"),
        None => title.to_string(),
    };
    let names = |key: &str| -> Vec<String> {
        book[key].as_array().into_iter().flatten().filter_map(|a| a["name"].as_str().map(String::from)).collect()
    };
    let year = book["publish_date"].as_str().and_then(year_in);
    let mut bib = Entry::new("book");
    bib.set("title", Some(&title));
    let authors = names("authors").join(" and ");
    bib.set("author", Some(authors.as_str()).filter(|a| !a.is_empty()));
    bib.set("publisher", names("publishers").first().map(String::as_str));
    bib.set("year", year.map(|y| y.to_string()).as_deref());
    bib.set("pagetotal", book["number_of_pages"].as_u64().map(|n| n.to_string()).as_deref());
    bib.set("isbn", Some(isbn));
    Some(bib)
}

/// The four-digit year in Open Library's free-form dates ("August 10, 1998", "1999", "2012-03").
fn year_in(date: &str) -> Option<i32> {
    date.as_bytes().windows(4).enumerate().find_map(|(i, w)| {
        let before = i.checked_sub(1).and_then(|j| date.as_bytes().get(j));
        let after = date.as_bytes().get(i + 4);
        let alone = !before.is_some_and(u8::is_ascii_digit) && !after.is_some_and(u8::is_ascii_digit);
        (alone && w.iter().all(u8::is_ascii_digit)).then(|| date[i..i + 4].parse().ok()).flatten()
    })
}

/// Titles of the same book: equal, or one the other with a subtitle or a leading "The" (Open Library
/// writes "The Fourier transform and its applications", a .bib "Fourier Transform and its applications").
pub(crate) fn same_book(a: &str, b: &str) -> bool {
    let (a, b) = (fetch::title_key(a), fetch::title_key(b));
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    short.len() >= 10 && (long.starts_with(&short) || long.strip_prefix("the").is_some_and(|l| l.starts_with(&short)))
}

/// The ISBN of the edition of `title` by `author` (a family name) published in `year`, from Open
/// Library; None when no edition of that year has one, since another edition's would be wrong.
pub fn find(title: &str, author: &str, year: i32) -> Result<Option<String>> {
    let search = format!(
        "{OPEN_LIBRARY}/search.json?title={}&author={}&fields=key,title&limit=5",
        encode(title),
        encode(author)
    );
    let v: Value = agent().get(&search).call().context("cannot reach Open Library")?.into_body().read_json()?;
    let works =
        v["docs"].as_array().into_iter().flatten().filter(|w| w["title"].as_str().is_some_and(|t| same_book(t, title)));
    for work in works.filter_map(|w| w["key"].as_str()) {
        let url = format!("{OPEN_LIBRARY}{work}/editions.json?limit=200");
        let editions: Value = agent().get(&url).call()?.into_body().read_json()?;
        if let Some(isbn) = edition_isbn(&editions, year) {
            return Ok(Some(isbn));
        }
    }
    Ok(None)
}

/// The first valid ISBN among the editions published in `year`.
fn edition_isbn(editions: &Value, year: i32) -> Option<String> {
    let entries = editions["entries"].as_array()?;
    let of_year = entries.iter().filter(|e| e["publish_date"].as_str().and_then(year_in) == Some(year));
    of_year
        .flat_map(|e| ["isbn_13", "isbn_10"].map(|k| e[k].clone()))
        .find_map(|list| list.as_array()?.iter().filter_map(Value::as_str).find_map(isbn13))
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// A book's ISBN: from its .bib, else its text, else Open Library by title, first author and year.
pub fn isbn_for(bib: &Entry, text: &str) -> Result<Option<String>> {
    if let Some(isbn) = bib.get("isbn").as_deref().and_then(isbn13) {
        return Ok(Some(isbn));
    }
    if let Some(isbn) = find_isbn(text) {
        return Ok(Some(isbn));
    }
    match (bib.authors().first(), bib.year()) {
        (Some(author), Some(year)) => find(&bib.title(), bib::family(author), year),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn isbns() {
        assert_eq!(isbn13("978-0-521-88349-8").as_deref(), Some("9780521883498"));
        assert_eq!(isbn13("0 521 78252 X").as_deref(), Some("9780521782524"), "ISBN-10 with X, as 13");
        assert_eq!(isbn13("0-471-30932-X").as_deref(), Some("9780471309321"));
        assert_eq!(isbn13("978-0-521-88349-7"), None, "wrong check digit");
        assert_eq!(isbn13("0-19-510691"), None, "too short");
        assert_eq!(isbn13("1234567890123"), None, "not 978 or 979");
    }

    #[test]
    fn isbns_in_text() {
        let find = |t: &str| find_isbn(t);
        assert_eq!(find("Printed in the UK\n\nISBN 978-0-521-88349-8 hardback").as_deref(), Some("9780521883498"));
        assert_eq!(find("ISBN-13 978-0-521-83490-2\nISBN-10 0-521-83490-2").as_deref(), Some("9780521834902"));
        assert_eq!(find("ISBN: 0-08-044371-0").as_deref(), Some("9780080443713"));
        assert_eq!(find("isbn 0 521 78252 X (hb)").as_deref(), Some("9780521782524"));
        assert_eq!(find("ISBN 0-521--33444-6").as_deref(), Some("9780521334440"), "a doubled hyphen, as printed");
        assert_eq!(find("ISBN 0-19-510691; ISBN 0-13-010997-5").as_deref(), Some("9780130109972"), "skips a cut one");
        assert_eq!(find("ISBN 978-3-662-56114-0 ISBN 978-3-662-56115-7 (eBook)").as_deref(), Some("9783662561140"));
        assert_eq!(find("call 0-521-78252-X, not an ISBN"), None, "only after the label");
        assert_eq!(find("ISBN 1234-5678 and ISSN 0019-1035"), None);
    }

    #[test]
    fn open_library() {
        let book = json!({"title": "Theory of reflectance", "subtitle": "and emittance",
            "authors": [{"name": "Bruce Hapke"}], "publishers": [{"name": "Cambridge University Press"}],
            "publish_date": "August 10, 2012", "number_of_pages": 513});
        let bib = entry(&book, "9780521883498").unwrap();
        assert_eq!(bib.kind, "book");
        assert_eq!(bib.title(), "Theory of reflectance: and emittance");
        assert_eq!((bib.get("author"), bib.year()), (Some("Bruce Hapke".into()), Some(2012)));
        assert_eq!(bib.get("publisher").as_deref(), Some("Cambridge University Press"));
        assert_eq!((bib.get("pagetotal").as_deref(), bib.get("isbn").as_deref()), (Some("513"), Some("9780521883498")));
        assert!(entry(&json!(null), "x").is_none(), "unknown ISBN");

        let editions = json!({"entries": [
            {"publish_date": "1975", "isbn_10": ["047143132X"]},
            {"publish_date": "1999", "isbn_13": ["9788126510948"]},
            {"publish_date": "August 10, 1998", "isbn_13": ["9780471309320", "9780471309321"]},
        ]});
        assert_eq!(edition_isbn(&editions, 1998).as_deref(), Some("9780471309321"), "the valid one of that year");
        assert_eq!(edition_isbn(&editions, 2001), None);
        assert_eq!(year_in("12345 and 1998"), Some(1998));
    }

    #[test]
    fn book_titles() {
        assert!(same_book("The Fourier transform and its applications", "Fourier Transform and its applications."));
        assert!(same_book("Theoretische Physik 1 | Mechanik", "Theoretische Physik 1: Mechanik"));
        assert!(same_book("Lunar sourcebook: a user's guide to the moon", "Lunar Sourcebook"));
        assert!(!same_book("Classical mechanics", "Classical electrodynamics"));
        assert!(!same_book("Optics", "Optics and lasers"), "too short to trust a prefix");
    }

    /// Open Library, live: Jackson's third edition.
    #[test]
    #[ignore = "network"]
    fn live() {
        assert_eq!(find("Classical electrodynamics", "Jackson", 1998).unwrap().as_deref(), Some("9780471309321"));
        let bib = by_isbn("9780521883498").unwrap().unwrap();
        assert!(bib.title().starts_with("Theory of reflectance"));
    }
}
