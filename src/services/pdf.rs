#![allow(dead_code)]

use crate::clients::webdav::WebDavClient;
use crate::clients::zotero::ZoteroClient;

/// Download a PDF from `pdf_url` and attach it to a Zotero item via WebDAV.
///
/// Stub — full implementation in a future task.
pub async fn download_and_attach_pdf(
    _client: &ZoteroClient,
    _parent_key: &str,
    _pdf_url: &str,
    _identifier: &str,
    _webdav: &WebDavClient,
    _filename: &str,
) -> anyhow::Result<()> {
    Ok(())
}

/// Attach a linked URL to a Zotero item as a child attachment.
///
/// Stub — full implementation in a future task.
pub async fn attach_linked_url(
    _client: &ZoteroClient,
    _parent_key: &str,
    _url: &str,
    _title: &str,
) -> anyhow::Result<()> {
    Ok(())
}
