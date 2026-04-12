#![allow(dead_code)]

use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use std::sync::OnceLock;

use crate::clients::webdav::WebDavClient;
use crate::clients::zotero::ZoteroClient;

use super::pdf::download_and_attach_pdf;

// ---------------------------------------------------------------------------
// URL encoding helper
// ---------------------------------------------------------------------------

/// Percent-encode a string for use in URI path/query segments.
fn encode_uri_component(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 2);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => {
                result.push(byte as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Unpaywall
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct UnpaywallOaLocation {
    url_for_pdf: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UnpaywallResponse {
    best_oa_location: Option<UnpaywallOaLocation>,
    oa_locations: Option<Vec<UnpaywallOaLocation>>,
}

/// Try to find an Open Access PDF URL via the Unpaywall API.
pub async fn try_unpaywall(doi: &str) -> Option<String> {
    let url = format!(
        "https://api.unpaywall.org/v2/{}?email=zotero-mcp@users.noreply.github.com",
        encode_uri_component(doi)
    );
    let resp = reqwest::get(&url).await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let data: UnpaywallResponse = resp.json().await.ok()?;

    // Try best OA location first
    if let Some(best) = &data.best_oa_location
        && let Some(pdf_url) = &best.url_for_pdf
        && !pdf_url.is_empty()
    {
        return Some(pdf_url.clone());
    }

    // Fall back to any oa_locations entry with a PDF URL
    if let Some(locations) = &data.oa_locations {
        for loc in locations {
            if let Some(pdf_url) = &loc.url_for_pdf
                && !pdf_url.is_empty()
            {
                return Some(pdf_url.clone());
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Semantic Scholar
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct S2OpenAccessPdf {
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct S2Response {
    open_access_pdf: Option<S2OpenAccessPdf>,
}

/// Try to find an Open Access PDF URL via the Semantic Scholar API.
pub async fn try_semantic_scholar(doi: &str) -> Option<String> {
    let url = format!(
        "https://api.semanticscholar.org/graph/v1/paper/DOI:{}?fields=openAccessPdf",
        encode_uri_component(doi)
    );
    let resp = reqwest::get(&url).await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let data: S2Response = resp.json().await.ok()?;
    data.open_access_pdf?.url
}

// ---------------------------------------------------------------------------
// PMC
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct PmcRecord {
    pmcid: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PmcIdConvResponse {
    records: Option<Vec<PmcRecord>>,
}

/// Try to find a PMC PDF URL by converting the DOI to a PMC ID.
pub async fn try_pmc(doi: &str) -> Option<String> {
    let url = format!(
        "https://pmc.ncbi.nlm.nih.gov/tools/idconv/api/v1/articles/?ids={}&format=json",
        encode_uri_component(doi)
    );
    let resp = reqwest::get(&url).await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let data: PmcIdConvResponse = resp.json().await.ok()?;
    let pmcid = data.records?.into_iter().next()?.pmcid?;
    if pmcid.is_empty() {
        return None;
    }
    Some(format!(
        "https://pmc.ncbi.nlm.nih.gov/articles/{}/pdf/",
        pmcid
    ))
}

// ---------------------------------------------------------------------------
// arXiv extraction from CrossRef relation metadata
// ---------------------------------------------------------------------------

fn arxiv_id_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\d{4}\.\d{4,5}(v\d+)?").unwrap())
}

/// Extract an arXiv ID from CrossRef relation metadata (the "is-preprint-of" field).
pub fn try_arxiv_from_crossref(metadata: &Value) -> Option<String> {
    let relation = metadata.get("relation")?;
    let alternates = relation.get("is-preprint-of")?.as_array()?;

    for candidate in alternates {
        let id = candidate.get("id")?.as_str()?;
        if id.to_lowercase().contains("arxiv")
            && let Some(m) = arxiv_id_regex().find(id)
        {
            return Some(m.as_str().to_string());
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Main OA PDF attachment function
// ---------------------------------------------------------------------------

/// Try to attach an Open Access PDF to a Zotero item.
///
/// Attempts sources in sequence: Unpaywall, Semantic Scholar, PMC, then
/// optionally arXiv (from crossref relations). Returns the first successful
/// PDF URL, or None.
pub async fn try_attach_oa_pdf(
    client: &ZoteroClient,
    item_key: &str,
    doi: &str,
    webdav: &WebDavClient,
    crossref_meta: Option<&Value>,
) -> Option<String> {
    // Build list of PDF URL futures
    let unpaywall = try_unpaywall(doi).await;
    if let Some(pdf_url) = &unpaywall
        && try_download(client, item_key, pdf_url, doi, webdav).await
    {
        return Some(pdf_url.clone());
    }

    let s2 = try_semantic_scholar(doi).await;
    if let Some(pdf_url) = &s2
        && try_download(client, item_key, pdf_url, doi, webdav).await
    {
        return Some(pdf_url.clone());
    }

    let pmc = try_pmc(doi).await;
    if let Some(pdf_url) = &pmc
        && try_download(client, item_key, pdf_url, doi, webdav).await
    {
        return Some(pdf_url.clone());
    }

    // Try arXiv from crossref relations
    if let Some(meta) = crossref_meta
        && let Some(arxiv_id) = try_arxiv_from_crossref(meta)
    {
        let pdf_url = format!("https://arxiv.org/pdf/{}.pdf", arxiv_id);
        if try_download(client, item_key, &pdf_url, doi, webdav).await {
            return Some(pdf_url);
        }
    }

    None
}

/// Attempt to download and attach a PDF; returns true on success.
async fn try_download(
    client: &ZoteroClient,
    item_key: &str,
    pdf_url: &str,
    doi: &str,
    webdav: &WebDavClient,
) -> bool {
    let filename = format!("{}.pdf", doi.replace('/', "_"));
    download_and_attach_pdf(client, item_key, pdf_url, doi, webdav, &filename)
        .await
        .is_ok()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_arxiv_id_from_crossref_relation() {
        let meta = json!({
            "relation": {
                "is-preprint-of": [
                    {
                        "id": "https://arxiv.org/abs/2301.12345v2",
                        "id-type": "uri"
                    }
                ]
            }
        });
        let result = try_arxiv_from_crossref(&meta);
        assert_eq!(result, Some("2301.12345v2".to_string()));
    }

    #[test]
    fn test_extract_arxiv_id_no_relation() {
        let meta = json!({});
        assert_eq!(try_arxiv_from_crossref(&meta), None);
    }

    #[test]
    fn test_extract_arxiv_id_no_preprint_of() {
        let meta = json!({
            "relation": {
                "cites": [{"id": "something"}]
            }
        });
        assert_eq!(try_arxiv_from_crossref(&meta), None);
    }

    #[test]
    fn test_extract_arxiv_id_non_arxiv() {
        let meta = json!({
            "relation": {
                "is-preprint-of": [
                    {
                        "id": "https://doi.org/10.1234/example",
                        "id-type": "uri"
                    }
                ]
            }
        });
        assert_eq!(try_arxiv_from_crossref(&meta), None);
    }

    #[test]
    fn test_extract_arxiv_id_multiple_candidates() {
        let meta = json!({
            "relation": {
                "is-preprint-of": [
                    {
                        "id": "https://doi.org/10.1234/example",
                        "id-type": "uri"
                    },
                    {
                        "id": "https://arxiv.org/abs/2405.67890",
                        "id-type": "uri"
                    }
                ]
            }
        });
        let result = try_arxiv_from_crossref(&meta);
        assert_eq!(result, Some("2405.67890".to_string()));
    }

    #[test]
    fn test_extract_arxiv_id_without_version() {
        let meta = json!({
            "relation": {
                "is-preprint-of": [
                    {
                        "id": "https://arxiv.org/abs/2301.12345",
                        "id-type": "uri"
                    }
                ]
            }
        });
        let result = try_arxiv_from_crossref(&meta);
        assert_eq!(result, Some("2301.12345".to_string()));
    }

    #[test]
    fn test_unpaywall_response_deserialize() {
        let json = r#"{
            "best_oa_location": {"url_for_pdf": "https://example.com/paper.pdf"},
            "oa_locations": [
                {"url_for_pdf": "https://example.com/alt.pdf"},
                {"url_for_pdf": null}
            ]
        }"#;
        let resp: UnpaywallResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            resp.best_oa_location.unwrap().url_for_pdf.as_deref(),
            Some("https://example.com/paper.pdf")
        );
        let locations = resp.oa_locations.unwrap();
        assert_eq!(locations.len(), 2);
        assert_eq!(
            locations[0].url_for_pdf.as_deref(),
            Some("https://example.com/alt.pdf")
        );
        assert_eq!(locations[1].url_for_pdf, None);
    }

    #[test]
    fn test_s2_response_deserialize() {
        let json = r#"{"openAccessPdf": {"url": "https://example.com/s2.pdf"}}"#;
        let resp: S2Response = serde_json::from_str(json).unwrap();
        assert_eq!(
            resp.open_access_pdf.unwrap().url.as_deref(),
            Some("https://example.com/s2.pdf")
        );
    }

    #[test]
    fn test_pmc_response_deserialize() {
        let json = r#"{"records": [{"pmcid": "PMC1234567"}]}"#;
        let resp: PmcIdConvResponse = serde_json::from_str(json).unwrap();
        let records = resp.records.unwrap();
        let pmcid = records[0].pmcid.as_deref();
        assert_eq!(pmcid, Some("PMC1234567"));
    }
}
