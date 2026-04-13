#![allow(dead_code)]

use phf::phf_map;
use regex::Regex;
use serde_json::Value;
use std::time::Duration;

use crate::clients::webdav::WebDavClient;
use crate::clients::zotero::ZoteroClient;
use crate::shared::formatters::format_item_result;
use crate::shared::template_engine::build_renamed_filename;
use crate::shared::types::{ZoteroItemData, ZoteroTag};
use crate::shared::validators::{extract_created_key, handle_write_response, parse_creator_names};

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum ArxivLookupError {
    #[error("arXiv is temporarily unavailable ({status}). Please retry zotero_add_paper shortly.")]
    TemporaryUnavailable { status: u16 },

    #[error("arXiv lookup failed: {message}")]
    PermanentError { message: String },
}

// Retry configuration
const ARXIV_MAX_RETRIES: usize = 3;
const ARXIV_RETRY_DELAYS: [Duration; 3] = [
    Duration::from_millis(250),
    Duration::from_millis(500),
    Duration::from_millis(1000),
];

// ---------------------------------------------------------------------------
// arXiv category map (compile-time, 184 entries)
// ---------------------------------------------------------------------------

pub static ARXIV_CATEGORIES: phf::Map<&'static str, &'static str> = phf_map! {
    // Main categories
    "cs" => "Computer Science",
    "econ" => "Economics",
    "eess" => "Electrical Engineering and Systems Science",
    "math" => "Mathematics",
    "nlin" => "Nonlinear Sciences",
    "physics" => "Physics",
    "q-fin" => "Quantitative Finance",
    "stat" => "Statistics",

    // Legacy / cross-listed
    "acc-phys" => "Accelerator Physics",
    "adap-org" => "Adaptation, Noise, and Self-Organizing Systems",
    "alg-geom" => "Algebraic Geometry",
    "ao-sci" => "Atmospheric-Oceanic Sciences",
    "astro-ph" => "Astrophysics",
    "astro-ph.CO" => "Cosmology and Nongalactic Astrophysics",
    "astro-ph.EP" => "Earth and Planetary Astrophysics",
    "astro-ph.GA" => "Astrophysics of Galaxies",
    "astro-ph.HE" => "High Energy Astrophysical Phenomena",
    "astro-ph.IM" => "Instrumentation and Methods for Astrophysics",
    "astro-ph.SR" => "Solar and Stellar Astrophysics",
    "atom-ph" => "Atomic, Molecular and Optical Physics",
    "bayes-an" => "Bayesian Analysis",
    "chao-dyn" => "Chaotic Dynamics",
    "chem-ph" => "Chemical Physics",
    "cmp-lg" => "Computation and Language",
    "comp-gas" => "Cellular Automata and Lattice Gases",
    "cond-mat" => "Condensed Matter",
    "cond-mat.dis-nn" => "Disordered Systems and Neural Networks",
    "cond-mat.mes-hall" => "Mesoscale and Nanoscale Physics",
    "cond-mat.mtrl-sci" => "Materials Science",
    "cond-mat.other" => "Other Condensed Matter",
    "cond-mat.quant-gas" => "Quantum Gases",
    "cond-mat.soft" => "Soft Condensed Matter",
    "cond-mat.stat-mech" => "Statistical Mechanics",
    "cond-mat.str-el" => "Strongly Correlated Electrons",
    "cond-mat.supr-con" => "Superconductivity",

    // Computer Science subcategories
    "cs.AI" => "Artificial Intelligence",
    "cs.AR" => "Hardware Architecture",
    "cs.CC" => "Computational Complexity",
    "cs.CE" => "Computational Engineering, Finance, and Science",
    "cs.CG" => "Computational Geometry",
    "cs.CL" => "Computation and Language",
    "cs.CR" => "Cryptography and Security",
    "cs.CV" => "Computer Vision and Pattern Recognition",
    "cs.CY" => "Computers and Society",
    "cs.DB" => "Databases",
    "cs.DC" => "Distributed, Parallel, and Cluster Computing",
    "cs.DL" => "Digital Libraries",
    "cs.DM" => "Discrete Mathematics",
    "cs.DS" => "Data Structures and Algorithms",
    "cs.ET" => "Emerging Technologies",
    "cs.FL" => "Formal Languages and Automata Theory",
    "cs.GL" => "General Literature",
    "cs.GR" => "Graphics",
    "cs.GT" => "Computer Science and Game Theory",
    "cs.HC" => "Human-Computer Interaction",
    "cs.IR" => "Information Retrieval",
    "cs.IT" => "Information Theory",
    "cs.LG" => "Machine Learning",
    "cs.LO" => "Logic in Computer Science",
    "cs.MA" => "Multiagent Systems",
    "cs.MM" => "Multimedia",
    "cs.MS" => "Mathematical Software",
    "cs.NA" => "Numerical Analysis",
    "cs.NE" => "Neural and Evolutionary Computing",
    "cs.NI" => "Networking and Internet Architecture",
    "cs.OH" => "Other Computer Science",
    "cs.OS" => "Operating Systems",
    "cs.PF" => "Performance",
    "cs.PL" => "Programming Languages",
    "cs.RO" => "Robotics",
    "cs.SC" => "Symbolic Computation",
    "cs.SD" => "Sound",
    "cs.SE" => "Software Engineering",
    "cs.SI" => "Social and Information Networks",
    "cs.SY" => "Systems and Control",

    // Other category groups
    "dg-ga" => "Differential Geometry",
    "econ.EM" => "Econometrics",
    "econ.GN" => "General Economics",
    "econ.TH" => "Theoretical Economics",
    "eess.AS" => "Audio and Speech Processing",
    "eess.IV" => "Image and Video Processing",
    "eess.SP" => "Signal Processing",
    "eess.SY" => "Systems and Control",
    "funct-an" => "Functional Analysis",
    "gr-qc" => "General Relativity and Quantum Cosmology",
    "hep-ex" => "High Energy Physics - Experiment",
    "hep-lat" => "High Energy Physics - Lattice",
    "hep-ph" => "High Energy Physics - Phenomenology",
    "hep-th" => "High Energy Physics - Theory",
    "math-ph" => "Mathematical Physics",

    // Mathematics subcategories
    "math.AC" => "Commutative Algebra",
    "math.AG" => "Algebraic Geometry",
    "math.AP" => "Analysis of PDEs",
    "math.AT" => "Algebraic Topology",
    "math.CA" => "Classical Analysis and ODEs",
    "math.CO" => "Combinatorics",
    "math.CT" => "Category Theory",
    "math.CV" => "Complex Variables",
    "math.DG" => "Differential Geometry",
    "math.DS" => "Dynamical Systems",
    "math.FA" => "Functional Analysis",
    "math.GM" => "General Mathematics",
    "math.GN" => "General Topology",
    "math.GR" => "Group Theory",
    "math.GT" => "Geometric Topology",
    "math.HO" => "History and Overview",
    "math.IT" => "Information Theory",
    "math.KT" => "K-Theory and Homology",
    "math.LO" => "Logic",
    "math.MG" => "Metric Geometry",
    "math.MP" => "Mathematical Physics",
    "math.NA" => "Numerical Analysis",
    "math.NT" => "Number Theory",
    "math.OA" => "Operator Algebras",
    "math.OC" => "Optimization and Control",
    "math.PR" => "Probability",
    "math.QA" => "Quantum Algebra",
    "math.RA" => "Rings and Algebras",
    "math.RT" => "Representation Theory",
    "math.SG" => "Symplectic Geometry",
    "math.SP" => "Spectral Theory",
    "math.ST" => "Statistics Theory",

    // More legacy
    "mtrl-th" => "Materials Theory",

    // Nonlinear Sciences subcategories
    "nlin.AO" => "Adaptation and Self-Organizing Systems",
    "nlin.CD" => "Chaotic Dynamics",
    "nlin.CG" => "Cellular Automata and Lattice Gases",
    "nlin.PS" => "Pattern Formation and Solitons",
    "nlin.SI" => "Exactly Solvable and Integrable Systems",

    // Nuclear
    "nucl-ex" => "Nuclear Experiment",
    "nucl-th" => "Nuclear Theory",
    "patt-sol" => "Pattern Formation and Solitons",

    // Physics subcategories
    "physics.acc-ph" => "Accelerator Physics",
    "physics.ao-ph" => "Atmospheric and Oceanic Physics",
    "physics.app-ph" => "Applied Physics",
    "physics.atm-clus" => "Atomic and Molecular Clusters",
    "physics.atom-ph" => "Atomic Physics",
    "physics.bio-ph" => "Biological Physics",
    "physics.chem-ph" => "Chemical Physics",
    "physics.class-ph" => "Classical Physics",
    "physics.comp-ph" => "Computational Physics",
    "physics.data-an" => "Data Analysis, Statistics and Probability",
    "physics.ed-ph" => "Physics Education",
    "physics.flu-dyn" => "Fluid Dynamics",
    "physics.gen-ph" => "General Physics",
    "physics.geo-ph" => "Geophysics",
    "physics.hist-ph" => "History and Philosophy of Physics",
    "physics.ins-det" => "Instrumentation and Detectors",
    "physics.med-ph" => "Medical Physics",
    "physics.optics" => "Optics",
    "physics.plasm-ph" => "Plasma Physics",
    "physics.pop-ph" => "Popular Physics",
    "physics.soc-ph" => "Physics and Society",
    "physics.space-ph" => "Space Physics",

    // More legacy / other
    "plasm-ph" => "Plasma Physics",
    "q-alg" => "Quantum Algebra and Topology",
    "q-bio" => "Quantitative Biology",
    "q-bio.BM" => "Biomolecules",
    "q-bio.CB" => "Cell Behavior",
    "q-bio.GN" => "Genomics",
    "q-bio.MN" => "Molecular Networks",
    "q-bio.NC" => "Neurons and Cognition",
    "q-bio.OT" => "Other Quantitative Biology",
    "q-bio.PE" => "Populations and Evolution",
    "q-bio.QM" => "Quantitative Methods",
    "q-bio.SC" => "Subcellular Processes",
    "q-bio.TO" => "Tissues and Organs",
    "q-fin.CP" => "Computational Finance",
    "q-fin.EC" => "Economics",
    "q-fin.GN" => "General Finance",
    "q-fin.MF" => "Mathematical Finance",
    "q-fin.PM" => "Portfolio Management",
    "q-fin.PR" => "Pricing of Securities",
    "q-fin.RM" => "Risk Management",
    "q-fin.ST" => "Statistical Finance",
    "q-fin.TR" => "Trading and Market Microstructure",
    "quant-ph" => "Quantum Physics",
    "solv-int" => "Exactly Solvable and Integrable Systems",
    "stat.AP" => "Applications",
    "stat.CO" => "Computation",
    "stat.ME" => "Methodology",
    "stat.ML" => "Machine Learning",
    "stat.OT" => "Other Statistics",
    "stat.TH" => "Statistics Theory",
    "supr-con" => "Superconductivity",
};

// ---------------------------------------------------------------------------
// ArxivParsed struct
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ArxivParsed {
    pub title: String,
    pub summary: String,
    pub authors: Vec<String>,
    pub published: String,
    pub updated: String,
    pub doi: String,
    pub abs_url: String,
    pub categories: Vec<String>,
    pub primary_category: String,
    pub comment: String,
}

// ---------------------------------------------------------------------------
// AddArxivResult
// ---------------------------------------------------------------------------

pub struct AddArxivResult {
    pub key: String,
    pub result: String,
}

// ---------------------------------------------------------------------------
// XML helpers
// ---------------------------------------------------------------------------

fn xml_unescape(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

fn first_xml_tag(source: &str, tag_name: &str) -> String {
    let escaped = regex::escape(tag_name);
    let pattern = format!(r"(?is)<{}[^>]*>(.*?)</{}>", escaped, escaped);
    Regex::new(&pattern)
        .unwrap()
        .captures(source)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// DOI normalization
// ---------------------------------------------------------------------------

fn normalize_doi_like(raw: &str) -> Result<String, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    let without_url = Regex::new(r"(?i)^https?://(dx\.)?doi\.org/")
        .unwrap()
        .replace(trimmed, "")
        .into_owned();
    let cleaned = Regex::new(r"(?i)^doi:\s*")
        .unwrap()
        .replace(&without_url, "")
        .trim()
        .to_string();
    if !Regex::new(r"^10\.\d{4,9}/.+").unwrap().is_match(&cleaned) {
        return Err("Invalid DOI");
    }
    Ok(cleaned)
}

// ---------------------------------------------------------------------------
// parse_arxiv_atom
// ---------------------------------------------------------------------------

pub fn parse_arxiv_atom(xml: &str) -> Result<ArxivParsed, String> {
    let entry_re = Regex::new(r"(?is)<entry[^>]*>(.*?)</entry>").unwrap();
    let entry = entry_re
        .captures(xml)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str())
        .unwrap_or("");

    if entry.is_empty() {
        return Err("No <entry> found in arXiv Atom XML".to_string());
    }

    let title = xml_unescape(&first_xml_tag(entry, "title"))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if title.is_empty() {
        return Err("Title is empty in arXiv entry".to_string());
    }

    let summary = xml_unescape(&first_xml_tag(entry, "summary"))
        .trim()
        .to_string();
    let published = first_xml_tag(entry, "published");
    let updated = first_xml_tag(entry, "updated");
    let abs_url = first_xml_tag(entry, "id");

    let doi_raw = {
        let d = first_xml_tag(entry, "arxiv:doi");
        if d.is_empty() {
            first_xml_tag(entry, "doi")
        } else {
            d
        }
    };
    let doi = normalize_doi_like(&doi_raw)
        .ok()
        .filter(|d| !d.is_empty())
        .map(|d| d.to_lowercase())
        .unwrap_or_default();

    // Authors
    let author_re =
        Regex::new(r"(?is)<author[^>]*>.*?<name[^>]*>(.*?)</name>.*?</author>").unwrap();
    let authors: Vec<String> = author_re
        .captures_iter(entry)
        .filter_map(|caps| {
            let name = xml_unescape(caps.get(1)?.as_str().trim())
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if name.is_empty() { None } else { Some(name) }
        })
        .collect();

    // Categories
    let category_re = Regex::new(r#"(?i)<category[^>]*term=["']([^"']+)["'][^>]*/?\s*>"#).unwrap();
    let categories: Vec<String> = category_re
        .captures_iter(entry)
        .filter_map(|caps| caps.get(1).map(|m| m.as_str().to_string()))
        .collect();

    // Primary category
    let primary_re =
        Regex::new(r#"(?i)<arxiv:primary_category[^>]*term=["']([^"']+)["'][^>]*/?\s*>"#).unwrap();
    let primary_category = primary_re
        .captures(entry)
        .and_then(|caps| caps.get(1).map(|m| m.as_str().to_string()))
        .or_else(|| categories.first().cloned())
        .unwrap_or_default();

    // Comment
    let comment_raw = first_xml_tag(entry, "arxiv:comment");
    let comment_raw = if comment_raw.is_empty() {
        first_xml_tag(entry, "comment")
    } else {
        comment_raw
    };
    let comment = xml_unescape(&comment_raw).trim().to_string();

    Ok(ArxivParsed {
        title,
        summary,
        authors,
        published,
        updated,
        doi,
        abs_url,
        categories,
        primary_category,
        comment,
    })
}

// ---------------------------------------------------------------------------
// map_arxiv_category
// ---------------------------------------------------------------------------

/// Map an arXiv category code to a human-readable name.
///
/// For subcategories like "cs.AI", returns "Computer Science - Artificial Intelligence".
/// For main categories like "cs", returns "Computer Science".
/// Returns None for unknown codes.
pub fn map_arxiv_category(code: &str) -> Option<String> {
    // If it has a dot, try to build "MainCategory - SubCategory"
    if let Some(dot_pos) = code.find('.') {
        let main = &code[..dot_pos];
        if let Some(main_name) = ARXIV_CATEGORIES.get(main)
            && let Some(sub_name) = ARXIV_CATEGORIES.get(code)
        {
            return Some(format!("{} - {}", main_name, sub_name));
        }
    }
    // Direct lookup for main categories or non-dotted legacy codes
    ARXIV_CATEGORIES.get(code).map(|name| name.to_string())
}

// ---------------------------------------------------------------------------
// extract_project_urls
// ---------------------------------------------------------------------------

pub fn extract_project_urls(text: &str) -> Vec<String> {
    let url_re = Regex::new(r#"https?://[^\s,;)<>\]"']+"#).unwrap();
    let context_re = Regex::new(
        r"(?i)\b(?:project\s*page|homepage|code|demo|website|webpage|supplementary|source\s*code)\b",
    )
    .unwrap();

    let excluded = ["arxiv.org", "doi.org", "dx.doi.org", "creativecommons.org"];
    let url_indicators = ["github.com", "github.io", "gitlab.com", "huggingface.co"];

    let mut seen = std::collections::HashSet::new();
    let mut urls = Vec::new();

    for m in url_re.find_iter(text) {
        let raw = m.as_str().trim_end_matches(['.', ')']);
        let lower = raw.to_lowercase();

        if excluded.iter().any(|d| lower.contains(d)) {
            continue;
        }
        if seen.contains(&lower) {
            continue;
        }

        let is_indicator = url_indicators.iter().any(|ind| lower.contains(ind));
        let context_start = m.start().saturating_sub(60);
        let surrounding = &text[context_start..m.start()];
        let has_context = context_re.is_match(surrounding);

        if is_indicator || has_context {
            seen.insert(lower);
            urls.push(raw.to_string());
        }
    }

    urls
}

// ---------------------------------------------------------------------------
// fetch_arxiv_atom_with_retry
// ---------------------------------------------------------------------------

/// Fetch arXiv Atom XML with retry logic for temporary failures.
/// Retries on 429 (Too Many Requests) and 503 (Service Unavailable).
pub async fn fetch_arxiv_atom_with_retry(
    url: &str,
    max_retries: usize,
) -> Result<String, ArxivLookupError> {
    let client = reqwest::Client::new();

    for attempt in 0..=max_retries {
        let response =
            client
                .get(url)
                .send()
                .await
                .map_err(|e| ArxivLookupError::PermanentError {
                    message: e.to_string(),
                })?;

        let status = response.status().as_u16();

        if response.status().is_success() {
            return response
                .text()
                .await
                .map_err(|e| ArxivLookupError::PermanentError {
                    message: e.to_string(),
                });
        }

        if matches!(status, 429 | 503) && attempt < max_retries {
            let delay = response
                .headers()
                .get("Retry-After")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .map(Duration::from_secs)
                .unwrap_or(ARXIV_RETRY_DELAYS[attempt.min(ARXIV_RETRY_DELAYS.len() - 1)]);

            tracing::warn!(
                "arXiv rate limited ({}), retrying in {:?} (attempt {}/{})",
                status,
                delay,
                attempt + 1,
                max_retries
            );

            tokio::time::sleep(delay).await;
            continue;
        }

        if matches!(status, 429 | 503) {
            return Err(ArxivLookupError::TemporaryUnavailable { status });
        }

        return Err(ArxivLookupError::PermanentError {
            message: format!("HTTP {}", status),
        });
    }

    Err(ArxivLookupError::TemporaryUnavailable { status: 429 })
}

// ---------------------------------------------------------------------------
// add_via_arxiv (full orchestration)
// ---------------------------------------------------------------------------

pub async fn add_via_arxiv(
    client: &ZoteroClient,
    webdav: &WebDavClient,
    arxiv_id: &str,
    collection_keys: Vec<String>,
    tags: Vec<ZoteroTag>,
) -> anyhow::Result<AddArxivResult> {
    // 1. Fetch arXiv Atom XML
    let url = format!(
        "https://export.arxiv.org/api/query?id_list={}",
        urlencoding(arxiv_id)
    );
    let xml = fetch_arxiv_atom_with_retry(&url, ARXIV_MAX_RETRIES)
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // 2. Parse XML
    let parsed = parse_arxiv_atom(&xml).map_err(|e| anyhow::anyhow!("arXiv parse error: {}", e))?;
    if parsed.title.is_empty() {
        anyhow::bail!("arXiv metadata not found");
    }

    // 3. Build category tags
    let category_tags: Vec<ZoteroTag> = parsed
        .categories
        .iter()
        .filter_map(|code| map_arxiv_category(code))
        .map(|name| ZoteroTag {
            tag: name,
            tag_type: None,
        })
        .collect();

    let mut all_tags = category_tags;
    all_tags.extend(tags);

    // 4. Compute article URLs and IDs
    let abs_url = if parsed.abs_url.is_empty() {
        format!("https://arxiv.org/abs/{}", arxiv_id)
    } else {
        Regex::new(r"v\d+$")
            .unwrap()
            .replace(&parsed.abs_url, "")
            .into_owned()
    };
    let article_id = Regex::new(r"/abs/(.+)$")
        .unwrap()
        .captures(&abs_url)
        .and_then(|caps| caps.get(1).map(|m| m.as_str().to_string()))
        .unwrap_or_else(|| arxiv_id.to_string());

    // 5. Build extra field
    let primary_field = Regex::new(r"^.+?:")
        .unwrap()
        .replace(&parsed.primary_category, "")
        .into_owned();
    let primary_field = Regex::new(r"\..+?$")
        .unwrap()
        .replace(&primary_field, "")
        .into_owned();
    let extra_field = if primary_field.is_empty() {
        String::new()
    } else {
        format!("[{}]", primary_field)
    };
    let extra_line = if article_id.contains('/') {
        format!("arXiv:{}", article_id)
    } else {
        format!("arXiv:{} {}", article_id, extra_field)
            .trim()
            .to_string()
    };

    // 6. Get item template and build item data
    let template = client.get_item_template("preprint", None).await?;
    let creators = parse_creator_names(parsed.authors.clone());

    let mut item_data = template.clone();
    item_data.item_type = "preprint".to_string();
    item_data.title = Some(parsed.title.clone());
    item_data.creators = Some(creators);
    item_data.date = Some(if !parsed.updated.is_empty() {
        parsed.updated[..10.min(parsed.updated.len())].to_string()
    } else if !parsed.published.is_empty() {
        parsed.published[..10.min(parsed.published.len())].to_string()
    } else {
        String::new()
    });
    item_data.abstract_note = Some(if parsed.summary.is_empty() {
        String::new()
    } else {
        parsed.summary.clone()
    });
    item_data.doi = Some(if parsed.doi.is_empty() {
        format!("10.48550/arXiv.{}", article_id)
    } else {
        parsed.doi.clone()
    });
    item_data.url = Some(abs_url.clone());
    item_data.extra = Some(extra_line);
    item_data.tags = Some(all_tags);
    item_data.collections = Some(collection_keys);

    // Set arXiv-specific fields when no real DOI
    if parsed.doi.is_empty() {
        if template.publisher.is_some() || template.extra_fields.contains_key("publisher") {
            item_data.publisher = Some("arXiv".to_string());
        }
        if template.extra_fields.contains_key("number") {
            item_data.extra_fields.insert(
                "number".into(),
                Value::String(format!("arXiv:{}", article_id)),
            );
        }
        if template.extra_fields.contains_key("archiveID") {
            item_data.extra_fields.insert(
                "archiveID".into(),
                Value::String(format!("arXiv:{}", article_id)),
            );
        }
    }

    // 7. Create item
    let create_resp = client.create_items(&[item_data.clone()]).await?;
    let write_resp_value = serde_json::to_value(&create_resp)?;
    let write_status = handle_write_response(&write_resp_value);
    if !write_status.ok || write_status.data.is_none() {
        anyhow::bail!("{}", write_status.message);
    }
    let created_key = extract_created_key(write_status.data.as_ref().unwrap())
        .ok_or_else(|| anyhow::anyhow!("Create succeeded but no item key was returned"))?;

    // 8. Download + attach PDF (best-effort)
    let pdf_filename = build_renamed_filename(&item_data, "pdf");
    let pdf_url = format!("https://arxiv.org/pdf/{}.pdf", arxiv_id);
    let pdf_id = if parsed.doi.is_empty() {
        arxiv_id.to_string()
    } else {
        parsed.doi.clone()
    };
    let mut attached = false;
    match crate::services::pdf::download_and_attach_pdf(
        client,
        &created_key,
        &pdf_url,
        &pdf_id,
        webdav,
        &pdf_filename,
    )
    .await
    {
        Ok(_) => attached = true,
        Err(e) => tracing::warn!("Failed to attach PDF for {}: {}", arxiv_id, e),
    }

    // 9. Attach linked URL for abs page
    if let Err(e) =
        crate::services::pdf::attach_linked_url(client, &created_key, &abs_url, "Snapshot").await
    {
        tracing::warn!("Failed to attach abs URL: {}", e);
    }

    // 10. Extract project URLs from summary, attach as linked URLs
    let project_urls = extract_project_urls(&parsed.summary);
    for project_url in &project_urls {
        if let Err(e) = crate::services::pdf::attach_linked_url(
            client,
            &created_key,
            project_url,
            "Project Page",
        )
        .await
        {
            tracing::warn!("Failed to attach project URL {}: {}", project_url, e);
        }
    }

    // 11. If comment, create note
    if !parsed.comment.is_empty() {
        let note_data = ZoteroItemData {
            item_type: "note".to_string(),
            parent_item: Some(created_key.clone()),
            note: Some(format!("<p>Comment: {}</p>", parsed.comment)),
            tags: Some(vec![]),
            ..Default::default()
        };
        if let Err(e) = client.create_items(&[note_data]).await {
            tracing::warn!("Failed to create comment note: {}", e);
        }
    }

    // 12. Build result
    let created_item = client.get_item(&created_key).await?;
    let mut lines = vec![format_item_result(&created_item, None, true)];
    if attached {
        lines.push(format!("Attached PDF: {}", pdf_filename));
    }
    lines.push(format!("Attached arXiv page: {}", abs_url));
    if !project_urls.is_empty() {
        lines.push(format!(
            "Attached project pages: {}",
            project_urls.join(", ")
        ));
    }
    if !parsed.comment.is_empty() {
        lines.push(format!("Added comment note: {}", parsed.comment));
    }

    Ok(AddArxivResult {
        key: created_key,
        result: lines.join("\n"),
    })
}

/// Simple percent-encoding for arXiv IDs (covers the characters that may appear).
fn urlencoding(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len() * 3);
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    encoded
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_ATOM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:arxiv="http://arxiv.org/schemas/atom">
<entry>
  <title>Attention Is All You Need</title>
  <id>http://arxiv.org/abs/1706.03762v5</id>
  <published>2017-06-12T17:57:34Z</published>
  <updated>2017-12-06T14:29:58Z</updated>
  <summary>The dominant sequence transduction models are based on complex recurrent neural networks.</summary>
  <author><name>Ashish Vaswani</name></author>
  <author><name>Noam Shazeer</name></author>
  <category term="cs.CL" scheme="http://arxiv.org/schemas/atom"/>
  <category term="cs.LG" scheme="http://arxiv.org/schemas/atom"/>
  <arxiv:primary_category term="cs.CL" scheme="http://arxiv.org/schemas/atom"/>
  <arxiv:comment>See the project page at https://github.com/tensorflow/tensor2tensor</arxiv:comment>
</entry>
</feed>"#;

    #[test]
    fn test_parse_arxiv_atom() {
        let parsed = parse_arxiv_atom(SAMPLE_ATOM).unwrap();
        assert_eq!(parsed.title, "Attention Is All You Need");
        assert_eq!(
            parsed.summary,
            "The dominant sequence transduction models are based on complex recurrent neural networks."
        );
        assert_eq!(parsed.authors, vec!["Ashish Vaswani", "Noam Shazeer"]);
        assert_eq!(parsed.published, "2017-06-12T17:57:34Z");
        assert_eq!(parsed.updated, "2017-12-06T14:29:58Z");
        assert_eq!(parsed.abs_url, "http://arxiv.org/abs/1706.03762v5");
        assert_eq!(parsed.categories, vec!["cs.CL", "cs.LG"]);
        assert_eq!(parsed.primary_category, "cs.CL");
        assert!(parsed.comment.contains("tensor2tensor"));
        assert!(parsed.doi.is_empty());
    }

    #[test]
    fn test_parse_arxiv_atom_no_entry() {
        let xml = r#"<?xml version="1.0"?><feed></feed>"#;
        let err = parse_arxiv_atom(xml).unwrap_err();
        assert!(err.contains("No <entry>"));
    }

    #[test]
    fn test_parse_arxiv_atom_empty_title() {
        let xml = r#"<feed><entry><title>  </title></entry></feed>"#;
        let err = parse_arxiv_atom(xml).unwrap_err();
        assert!(err.contains("Title is empty"));
    }

    #[test]
    fn test_map_arxiv_category_exact() {
        assert_eq!(
            map_arxiv_category("cs"),
            Some("Computer Science".to_string())
        );
        assert_eq!(map_arxiv_category("math"), Some("Mathematics".to_string()));
        assert_eq!(map_arxiv_category("stat"), Some("Statistics".to_string()));
        assert_eq!(
            map_arxiv_category("gr-qc"),
            Some("General Relativity and Quantum Cosmology".to_string())
        );
    }

    #[test]
    fn test_map_arxiv_category_subcategory() {
        assert_eq!(
            map_arxiv_category("cs.AI"),
            Some("Computer Science - Artificial Intelligence".to_string())
        );
        assert_eq!(
            map_arxiv_category("cs.CL"),
            Some("Computer Science - Computation and Language".to_string())
        );
        assert_eq!(
            map_arxiv_category("math.AG"),
            Some("Mathematics - Algebraic Geometry".to_string())
        );
        assert_eq!(
            map_arxiv_category("stat.ML"),
            Some("Statistics - Machine Learning".to_string())
        );
    }

    #[test]
    fn test_map_arxiv_category_unknown() {
        assert_eq!(map_arxiv_category("xyz.ZZ"), None);
        assert_eq!(map_arxiv_category("totally-unknown"), None);
    }

    #[test]
    fn test_map_arxiv_category_unknown_subcategory() {
        // Known main category but unknown subcategory → None (no fallback to main)
        assert_eq!(map_arxiv_category("cs.ZZ"), None);
    }

    #[test]
    fn test_extract_project_urls() {
        let text = "See the project page at https://github.com/tensorflow/tensor2tensor for code.";
        let urls = extract_project_urls(text);
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("github.com/tensorflow/tensor2tensor"));
    }

    #[test]
    fn test_extract_project_urls_filters_arxiv() {
        let text = "See https://arxiv.org/abs/1234 and https://github.com/user/repo";
        let urls = extract_project_urls(text);
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("github.com"));
    }

    #[test]
    fn test_extract_project_urls_empty() {
        let text = "This paper has no URLs.";
        let urls = extract_project_urls(text);
        assert!(urls.is_empty());
    }

    #[test]
    fn test_extract_project_urls_context_keywords() {
        let text = "Our code is available at https://example.com/project";
        let urls = extract_project_urls(text);
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("example.com/project"));
    }

    #[test]
    fn test_extract_project_urls_huggingface() {
        let text = "Model at https://huggingface.co/org/model";
        let urls = extract_project_urls(text);
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("huggingface.co"));
    }

    #[test]
    fn test_xml_unescape() {
        assert_eq!(xml_unescape("&lt;b&gt;test&lt;/b&gt;"), "<b>test</b>");
        assert_eq!(xml_unescape("&amp;"), "&");
        assert_eq!(xml_unescape("&quot;hi&quot;"), "\"hi\"");
        assert_eq!(xml_unescape("it&#39;s"), "it's");
        assert_eq!(xml_unescape("no entities"), "no entities");
    }

    #[test]
    fn test_first_xml_tag() {
        let xml = "<root><title>Hello World</title></root>";
        assert_eq!(first_xml_tag(xml, "title"), "Hello World");
    }

    #[test]
    fn test_first_xml_tag_with_attrs() {
        let xml = r#"<root><title lang="en">Hello</title></root>"#;
        assert_eq!(first_xml_tag(xml, "title"), "Hello");
    }

    #[test]
    fn test_first_xml_tag_missing() {
        let xml = "<root><other>data</other></root>";
        assert_eq!(first_xml_tag(xml, "title"), "");
    }

    #[test]
    fn test_first_xml_tag_namespaced() {
        let xml = r#"<root><arxiv:doi>10.1234/test</arxiv:doi></root>"#;
        assert_eq!(first_xml_tag(xml, "arxiv:doi"), "10.1234/test");
    }

    #[test]
    fn test_normalize_doi_like_valid() {
        assert_eq!(normalize_doi_like("10.1234/test").unwrap(), "10.1234/test");
        assert_eq!(
            normalize_doi_like("https://doi.org/10.1234/test").unwrap(),
            "10.1234/test"
        );
        assert_eq!(
            normalize_doi_like("doi: 10.1234/test").unwrap(),
            "10.1234/test"
        );
    }

    #[test]
    fn test_normalize_doi_like_invalid() {
        assert!(normalize_doi_like("not-a-doi").is_err());
    }

    #[test]
    fn test_normalize_doi_like_empty() {
        assert_eq!(normalize_doi_like("").unwrap(), "");
        assert_eq!(normalize_doi_like("   ").unwrap(), "");
    }

    #[test]
    fn test_category_count() {
        // Verify we have a substantial number of categories
        assert!(ARXIV_CATEGORIES.len() >= 180);
    }

    #[test]
    fn test_urlencoding() {
        assert_eq!(urlencoding("1706.03762"), "1706.03762");
        assert_eq!(urlencoding("hep-th/9901001"), "hep-th%2F9901001");
    }

    #[test]
    fn test_parse_arxiv_atom_with_doi() {
        let xml = r#"<feed><entry>
  <title>Paper With DOI</title>
  <id>http://arxiv.org/abs/2301.00001v1</id>
  <published>2023-01-01T00:00:00Z</published>
  <updated>2023-01-02T00:00:00Z</updated>
  <summary>A paper.</summary>
  <author><name>Jane Doe</name></author>
  <category term="cs.LG" scheme="http://arxiv.org/schemas/atom"/>
  <arxiv:primary_category term="cs.LG" scheme="http://arxiv.org/schemas/atom"/>
  <arxiv:doi>10.1234/example.2023</arxiv:doi>
</entry></feed>"#;
        let parsed = parse_arxiv_atom(xml).unwrap();
        assert_eq!(parsed.doi, "10.1234/example.2023");
        assert_eq!(parsed.authors, vec!["Jane Doe"]);
        assert_eq!(parsed.primary_category, "cs.LG");
    }

    #[test]
    fn test_parse_arxiv_atom_multiline_title() {
        let xml = r#"<feed><entry>
  <title>
    A Very Long Title That
    Spans Multiple Lines
  </title>
  <id>http://arxiv.org/abs/0001.0001v1</id>
  <published>2020-01-01T00:00:00Z</published>
  <updated>2020-01-01T00:00:00Z</updated>
  <summary>Abstract.</summary>
  <author><name>Test Author</name></author>
  <category term="math.CO" scheme="http://arxiv.org/schemas/atom"/>
  <arxiv:primary_category term="math.CO" scheme="http://arxiv.org/schemas/atom"/>
</entry></feed>"#;
        let parsed = parse_arxiv_atom(xml).unwrap();
        assert_eq!(parsed.title, "A Very Long Title That Spans Multiple Lines");
    }
}
