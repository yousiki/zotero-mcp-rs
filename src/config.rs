use std::env;

#[derive(Debug, Clone, PartialEq)]
pub enum TransportMode {
    Stdio,
    Http,
}

impl Default for TransportMode {
    fn default() -> Self {
        TransportMode::Stdio
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub zotero_api_key: String,
    pub zotero_library_id: Option<String>,
    pub zotero_library_type: String,
    pub webdav_url: Option<String>,
    pub webdav_username: Option<String>,
    pub webdav_password: Option<String>,
    pub port: u16,
    pub transport: TransportMode,
}

impl Config {
    /// Load configuration from environment variables.
    /// Returns Err if ZOTERO_API_KEY is not set or empty.
    pub fn from_env() -> Result<Self, String> {
        let api_key = env::var("ZOTERO_API_KEY").unwrap_or_default();
        if api_key.trim().is_empty() {
            return Err("ZOTERO_API_KEY environment variable is required".to_string());
        }

        let library_id = env::var("ZOTERO_LIBRARY_ID")
            .ok()
            .filter(|s| !s.trim().is_empty());
        let library_type = env::var("ZOTERO_LIBRARY_TYPE")
            .unwrap_or_else(|_| "user".to_string())
            .to_lowercase();
        let library_type = if library_type == "group" {
            "group".to_string()
        } else {
            "user".to_string()
        };

        let webdav_url = env::var("WEBDAV_URL").ok().filter(|s| !s.trim().is_empty());
        let webdav_username = env::var("WEBDAV_USERNAME")
            .ok()
            .filter(|s| !s.trim().is_empty());
        let webdav_password = env::var("WEBDAV_PASSWORD")
            .ok()
            .filter(|s| !s.trim().is_empty());

        let port = env::var("PORT")
            .ok()
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(3000);

        let transport = match env::var("MCP_TRANSPORT").as_deref() {
            Ok("http") => TransportMode::Http,
            Ok("stdio") | _ => TransportMode::Stdio,
        };

        Ok(Config {
            zotero_api_key: api_key,
            zotero_library_id: library_id,
            zotero_library_type: library_type,
            webdav_url,
            webdav_username,
            webdav_password,
            port,
            transport,
        })
    }

    /// Check if WebDAV is fully configured (all three fields present)
    pub fn webdav_configured(&self) -> bool {
        self.webdav_url.is_some()
            && self.webdav_username.is_some()
            && self.webdav_password.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    fn clear_env() {
        for key in &[
            "ZOTERO_API_KEY",
            "ZOTERO_LIBRARY_ID",
            "ZOTERO_LIBRARY_TYPE",
            "WEBDAV_URL",
            "WEBDAV_USERNAME",
            "WEBDAV_PASSWORD",
            "PORT",
            "MCP_TRANSPORT",
        ] {
            env::remove_var(key);
        }
    }

    #[test]
    fn test_missing_api_key_returns_error() {
        let _guard = env_guard();
        clear_env();
        let result = Config::from_env();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("ZOTERO_API_KEY"));
    }

    #[test]
    fn test_defaults() {
        let _guard = env_guard();
        clear_env();
        env::set_var("ZOTERO_API_KEY", "test_key_123");
        let config = Config::from_env().unwrap();
        assert_eq!(config.zotero_api_key, "test_key_123");
        assert_eq!(config.zotero_library_id, None);
        assert_eq!(config.zotero_library_type, "user");
        assert_eq!(config.port, 3000);
        assert_eq!(config.transport, TransportMode::Stdio);
        assert!(!config.webdav_configured());
        clear_env();
    }

    #[test]
    fn test_library_type_group() {
        let _guard = env_guard();
        clear_env();
        env::set_var("ZOTERO_API_KEY", "key");
        env::set_var("ZOTERO_LIBRARY_TYPE", "group");
        let config = Config::from_env().unwrap();
        assert_eq!(config.zotero_library_type, "group");
        clear_env();
    }

    #[test]
    fn test_library_type_defaults_to_user() {
        let _guard = env_guard();
        clear_env();
        env::set_var("ZOTERO_API_KEY", "key");
        env::set_var("ZOTERO_LIBRARY_TYPE", "GROUP"); // uppercase normalized
        let config = Config::from_env().unwrap();
        assert_eq!(config.zotero_library_type, "group");
        clear_env();
    }

    #[test]
    fn test_port_and_transport() {
        let _guard = env_guard();
        clear_env();
        env::set_var("ZOTERO_API_KEY", "key");
        env::set_var("PORT", "8080");
        env::set_var("MCP_TRANSPORT", "http");
        let config = Config::from_env().unwrap();
        assert_eq!(config.port, 8080);
        assert_eq!(config.transport, TransportMode::Http);
        clear_env();
    }

    #[test]
    fn test_webdav_configured() {
        let _guard = env_guard();
        clear_env();
        env::set_var("ZOTERO_API_KEY", "key");
        env::set_var("WEBDAV_URL", "https://dav.example.com");
        env::set_var("WEBDAV_USERNAME", "user");
        env::set_var("WEBDAV_PASSWORD", "pass");
        let config = Config::from_env().unwrap();
        assert!(config.webdav_configured());
        assert_eq!(
            config.webdav_url.as_deref(),
            Some("https://dav.example.com")
        );
        clear_env();
    }
}
