use anyhow::{Result, anyhow};
use base64::{Engine, engine::general_purpose};
use md5::{Digest, Md5};
use reqwest::Client;
use std::io::Write;
use zip::write::SimpleFileOptions;

pub struct UploadResult {
    pub md5: String,
    pub mtime: u64,
}

#[derive(Clone)]
pub struct WebDavClient {
    base_url: String,
    auth_header: String,
    http: Client,
}

impl WebDavClient {
    pub fn new(url: &str, username: &str, password: &str) -> Self {
        Self::with_client(url, username, password, Client::new())
    }

    fn with_client(url: &str, username: &str, password: &str, http: Client) -> Self {
        let base_url = url.trim_end_matches('/').to_string();
        let credentials = format!("{}:{}", username, password);
        let auth_header = format!("Basic {}", general_purpose::STANDARD.encode(credentials));
        Self {
            base_url,
            auth_header,
            http,
        }
    }

    /// Upload a file to WebDAV: ZIP the data, PUT as {key}.zip, PUT .prop with MD5+mtime.
    pub async fn upload_file(
        &self,
        key: &str,
        filename: &str,
        data: &[u8],
    ) -> Result<UploadResult> {
        let zip_data = create_zip(filename, data)?;

        let md5 = compute_md5(data);
        let mtime = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // PUT {key}.zip
        let zip_url = format!("{}/{}.zip", self.base_url, key);
        let resp = self
            .http
            .put(&zip_url)
            .header("Authorization", &self.auth_header)
            .header("Content-Type", "application/zip")
            .body(zip_data)
            .send()
            .await?;
        if !resp.status().is_success()
            && resp.status().as_u16() != 201
            && resp.status().as_u16() != 204
        {
            return Err(anyhow!("WebDAV PUT {}.zip failed ({})", key, resp.status()));
        }

        // PUT {key}.prop with XML metadata
        let prop_xml = format!(
            "<properties version=\"1\">\n  <mtime>{}</mtime>\n  <hash>{}</hash>\n</properties>",
            mtime, md5
        );
        let prop_url = format!("{}/{}.prop", self.base_url, key);
        let resp = self
            .http
            .put(&prop_url)
            .header("Authorization", &self.auth_header)
            .header("Content-Type", "text/xml")
            .body(prop_xml)
            .send()
            .await?;
        if !resp.status().is_success()
            && resp.status().as_u16() != 201
            && resp.status().as_u16() != 204
        {
            return Err(anyhow!(
                "WebDAV PUT {}.prop failed ({})",
                key,
                resp.status()
            ));
        }

        Ok(UploadResult { md5, mtime })
    }

    #[allow(dead_code)]
    /// Check if {key}.zip exists via HEAD request.
    pub async fn file_exists(&self, key: &str) -> Result<bool> {
        let url = format!("{}/{}.zip", self.base_url, key);
        let resp = self
            .http
            .head(&url)
            .header("Authorization", &self.auth_header)
            .send()
            .await?;
        Ok(resp.status().is_success())
    }

    #[allow(dead_code)]
    /// Delete {key}.zip and {key}.prop.
    pub async fn delete_file(&self, key: &str) -> Result<()> {
        let zip_url = format!("{}/{}.zip", self.base_url, key);
        self.http
            .delete(&zip_url)
            .header("Authorization", &self.auth_header)
            .send()
            .await?;

        let prop_url = format!("{}/{}.prop", self.base_url, key);
        self.http
            .delete(&prop_url)
            .header("Authorization", &self.auth_header)
            .send()
            .await?;

        Ok(())
    }

    #[allow(dead_code)]
    /// Verify WebDAV server is accessible via OPTIONS request with 5s timeout.
    pub async fn verify(&self) -> bool {
        match self
            .http
            .request(reqwest::Method::OPTIONS, &self.base_url)
            .header("Authorization", &self.auth_header)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
        {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }
}

fn create_zip(filename: &str, data: &[u8]) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut buf);
        let mut zip = zip::ZipWriter::new(cursor);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file(filename, options)?;
        zip.write_all(data)?;
        zip.finish()?;
    }
    Ok(buf)
}

fn compute_md5(data: &[u8]) -> String {
    let mut hasher = Md5::new();
    hasher.update(data);
    hasher.finalize().into_iter().map(|b| format!("{:02x}", b)).collect::<String>()
}

pub fn create_webdav_client(
    url: Option<&str>,
    username: Option<&str>,
    password: Option<&str>,
) -> Option<WebDavClient> {
    match (url, username, password) {
        (Some(u), Some(n), Some(p)) if !u.is_empty() && !n.is_empty() && !p.is_empty() => {
            Some(WebDavClient::new(u, n, p))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Build a test client that bypasses system proxy (needed for wiremock on localhost).
    fn test_client(server: &MockServer, username: &str, password: &str) -> WebDavClient {
        let http = Client::builder().no_proxy().build().unwrap();
        WebDavClient::with_client(&server.uri(), username, password, http)
    }

    #[tokio::test]
    async fn test_upload_creates_zip_and_prop() {
        let server = MockServer::start().await;

        Mock::given(method("PUT"))
            .and(path("/ABCD1234.zip"))
            .respond_with(ResponseTemplate::new(201))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/ABCD1234.prop"))
            .respond_with(ResponseTemplate::new(201))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(&server, "user", "pass");
        let result = client
            .upload_file("ABCD1234", "test.pdf", b"hello world")
            .await
            .unwrap();

        assert!(!result.md5.is_empty());
        assert!(result.mtime > 0);
        // MD5 of "hello world"
        assert_eq!(result.md5, "5eb63bbbe01eeed093cb22bb8f5acdc3");
    }

    #[tokio::test]
    async fn test_file_exists_head_request() {
        let server = MockServer::start().await;

        Mock::given(method("HEAD"))
            .and(path("/KEY12345.zip"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(&server, "user", "pass");
        let exists = client.file_exists("KEY12345").await.unwrap();
        assert!(exists);
    }

    #[tokio::test]
    async fn test_file_not_exists() {
        let server = MockServer::start().await;

        Mock::given(method("HEAD"))
            .and(path("/MISSING1.zip"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(&server, "user", "pass");
        let exists = client.file_exists("MISSING1").await.unwrap();
        assert!(!exists);
    }

    #[tokio::test]
    async fn test_auth_header_present() {
        let server = MockServer::start().await;
        let expected_auth = format!(
            "Basic {}",
            general_purpose::STANDARD.encode("testuser:testpass")
        );

        Mock::given(method("PUT"))
            .and(path("/AUTHTEST.zip"))
            .and(header("Authorization", expected_auth.as_str()))
            .respond_with(ResponseTemplate::new(201))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/AUTHTEST.prop"))
            .respond_with(ResponseTemplate::new(201))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(&server, "testuser", "testpass");
        client
            .upload_file("AUTHTEST", "doc.pdf", b"data")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_delete_file() {
        let server = MockServer::start().await;

        Mock::given(method("DELETE"))
            .and(path("/DEL12345.zip"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("DELETE"))
            .and(path("/DEL12345.prop"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(&server, "user", "pass");
        client.delete_file("DEL12345").await.unwrap();
    }

    #[tokio::test]
    async fn test_verify_success() {
        let server = MockServer::start().await;

        Mock::given(method("OPTIONS"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(&server, "user", "pass");
        assert!(client.verify().await);
    }

    #[tokio::test]
    async fn test_create_webdav_client_some() {
        let client = create_webdav_client(
            Some("https://dav.example.com/zotero"),
            Some("user"),
            Some("pass"),
        );
        assert!(client.is_some());
    }

    #[tokio::test]
    async fn test_create_webdav_client_none_on_missing() {
        assert!(create_webdav_client(None, Some("u"), Some("p")).is_none());
        assert!(create_webdav_client(Some("url"), None, Some("p")).is_none());
        assert!(create_webdav_client(Some("url"), Some("u"), None).is_none());
        assert!(create_webdav_client(Some(""), Some("u"), Some("p")).is_none());
    }
}
