use anyhow::{Result, anyhow};
use regex::Regex;

use crate::clients::zotero::ZoteroClient;
use crate::shared::types::ZoteroItem;
use std::collections::HashMap;
/// Canonical input type for identifier detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputType {
    Doi,
    Arxiv,
    Isbn,
    Url,
    File,
}

/// Normalize a DOI string to canonical form.
/// Handles plain DOI, URL forms, `doi:` prefixes, and percent-encoded input.
pub fn normalize_doi(raw: &str) -> Result<String> {
    let decoded = percent_decode(raw.trim());
    let re = Regex::new(r"(?i)10\.\d{4,9}/[\w.()\-;/:]+")?;
    let doi = re
        .find(&decoded)
        .map(|m| {
            m.as_str()
                .trim_end_matches(['>', ']', ')', '}', '.', ',', ';'])
        })
        .ok_or_else(|| anyhow!("Invalid DOI"))?;
    Ok(doi.to_string())
}

/// Normalize an arXiv ID to canonical form without version suffix.
pub fn normalize_arxiv_id(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    let re = Regex::new(
        r"(?i)(?:arxiv:|https?://arxiv\.org/(?:abs|pdf)/)?([a-z-]+/\d{7}|\d{4}\.\d{4,5})(?:v\d+)?",
    )?;
    let caps = re
        .captures(trimmed)
        .ok_or_else(|| anyhow!("Invalid arXiv ID"))?;
    Ok(caps.get(1).unwrap().as_str().to_lowercase())
}

/// Detect the type of an input string.
pub fn detect_input_type(input: &str) -> InputType {
    let trimmed = input.trim();

    if normalize_doi(trimmed).is_ok() {
        return InputType::Doi;
    }

    if normalize_arxiv_id(trimmed).is_ok() {
        return InputType::Arxiv;
    }

    let isbn_like: String = trimmed
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();
    if (isbn_like.len() == 13
        && (isbn_like.starts_with("978") || isbn_like.starts_with("979"))
        && isbn_like.chars().all(|c| c.is_ascii_digit()))
        || (isbn_like.len() == 10
            && isbn_like.chars().take(9).all(|c| c.is_ascii_digit())
            && isbn_like
                .chars()
                .last()
                .is_some_and(|c| c.is_ascii_digit() || matches!(c, 'X' | 'x')))
    {
        return InputType::Isbn;
    }

    if trimmed.starts_with('/') || trimmed.starts_with('~') || trimmed.starts_with("./") {
        return InputType::File;
    }

    if Regex::new(r"(?i)^[a-z]+://").unwrap().is_match(trimmed) {
        return InputType::Url;
    }

    InputType::Url
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2]))
        {
            out.push((h * 16 + l) as char);
            i += 3;
            continue;
        }

        out.push(bytes[i] as char);
        i += 1;
    }

    out
}

fn hex_val(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Stub for Zotero client-dependent lookup.
pub async fn find_existing_by_doi(_doi: &str) -> Option<ZoteroItem> {
    None
}

/// Stub for Zotero client-dependent lookup.
pub async fn find_existing_by_arxiv_id(_arxiv_id: &str) -> Option<ZoteroItem> {
    None
}

/// Resolve collection names to keys by fetching all collections and matching names.
/// Supports exact match and case-insensitive substring match.
pub async fn resolve_collection_names(client: &ZoteroClient, names: &[String]) -> Vec<String> {
    let collections = match client.get_collections(HashMap::new()).await {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let mut result = Vec::new();
    for name in names {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Try exact match first
        if let Some(c) = collections.iter().find(|c| c.data.name == trimmed) {
            result.push(c.key.clone());
            continue;
        }
        // Try case-insensitive match
        if let Some(c) = collections
            .iter()
            .find(|c| c.data.name.to_lowercase() == trimmed.to_lowercase())
        {
            result.push(c.key.clone());
            continue;
        }
        // Try substring match (case-insensitive)
        if let Some(c) = collections
            .iter()
            .find(|c| c.data.name.to_lowercase().contains(&trimmed.to_lowercase()))
        {
            result.push(c.key.clone());
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_doi_plain() {
        assert_eq!(
            normalize_doi("10.1038/s41586-020-2649-2").unwrap(),
            "10.1038/s41586-020-2649-2"
        );
    }

    #[test]
    fn normalize_doi_url() {
        assert_eq!(
            normalize_doi("https://doi.org/10.1038/s41586-020-2649-2").unwrap(),
            "10.1038/s41586-020-2649-2"
        );
    }

    #[test]
    fn normalize_doi_prefix() {
        assert_eq!(
            normalize_doi("doi:10.1038/s41586-020-2649-2").unwrap(),
            "10.1038/s41586-020-2649-2"
        );
    }

    #[test]
    fn normalize_doi_decodes_percent_encoding() {
        assert_eq!(
            normalize_doi("https://doi.org/10.1000%2F182").unwrap(),
            "10.1000/182"
        );
    }

    #[test]
    fn normalize_doi_strips_trailing_punctuation() {
        assert_eq!(normalize_doi("10.1234/test.).").unwrap(), "10.1234/test");
    }

    #[test]
    fn normalize_doi_invalid() {
        assert!(normalize_doi("").is_err());
        assert!(normalize_doi("not-a-doi").is_err());
        assert!(normalize_doi("https://example.com/paper").is_err());
    }

    #[test]
    fn normalize_arxiv_new_style() {
        assert_eq!(normalize_arxiv_id("2301.07041").unwrap(), "2301.07041");
    }

    #[test]
    fn normalize_arxiv_version_suffix() {
        assert_eq!(normalize_arxiv_id("2301.07041v2").unwrap(), "2301.07041");
    }

    #[test]
    fn normalize_arxiv_old_style() {
        assert_eq!(
            normalize_arxiv_id("hep-ph/0001234").unwrap(),
            "hep-ph/0001234"
        );
    }

    #[test]
    fn normalize_arxiv_url() {
        assert_eq!(
            normalize_arxiv_id("https://arxiv.org/abs/2301.07041v2").unwrap(),
            "2301.07041"
        );
    }

    #[test]
    fn normalize_arxiv_prefix() {
        assert_eq!(
            normalize_arxiv_id("arxiv:hep-ph/0001234v3").unwrap(),
            "hep-ph/0001234"
        );
    }

    #[test]
    fn normalize_arxiv_invalid() {
        assert!(normalize_arxiv_id("not-arxiv").is_err());
    }

    #[test]
    fn detect_doi() {
        assert_eq!(detect_input_type("10.1038/nature123"), InputType::Doi);
        assert_eq!(
            detect_input_type("https://doi.org/10.1234/test"),
            InputType::Doi
        );
    }

    #[test]
    fn detect_arxiv() {
        assert_eq!(detect_input_type("2301.07041"), InputType::Arxiv);
        assert_eq!(
            detect_input_type("https://arxiv.org/abs/2301.07041"),
            InputType::Arxiv
        );
    }

    #[test]
    fn detect_isbn13() {
        assert_eq!(detect_input_type("978-0-13-468599-1"), InputType::Isbn);
        assert_eq!(detect_input_type("9790134685997"), InputType::Isbn);
    }

    #[test]
    fn detect_isbn10() {
        assert_eq!(detect_input_type("0-13-468599-0"), InputType::Isbn);
        assert_eq!(detect_input_type("0134685997"), InputType::Isbn);
    }

    #[test]
    fn detect_file() {
        assert_eq!(detect_input_type("/path/to/file.pdf"), InputType::File);
        assert_eq!(detect_input_type("~/Documents/paper.pdf"), InputType::File);
        assert_eq!(detect_input_type("./paper.pdf"), InputType::File);
    }

    #[test]
    fn detect_url() {
        assert_eq!(
            detect_input_type("https://example.com/paper"),
            InputType::Url
        );
        assert_eq!(
            detect_input_type("http://journal.org/article"),
            InputType::Url
        );
    }
}
