use figment::{
    providers::{Env, Format, Serialized, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub retry: RetryConfig,
    pub lease: LeaseConfig,
    pub external_api: ExternalApiConfig,
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub data_dir: PathBuf,
    /// Unix domain socket path (Unix/macOS only — ignored on Windows).
    pub socket_path: PathBuf,
    /// TCP port used on Windows (ignored on Unix).
    pub port: u16,
    pub max_concurrent_calculations: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub max_attempts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaseConfig {
    pub heartbeat_interval_s: u64,
    pub expiry_s: u64,
    pub watchdog_interval_s: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalApiConfig {
    pub base_url: String,
    pub request_timeout_s: u64,
    pub supports_idempotency: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub file_level: String,
    pub stderr_level: String,
    pub file_path: PathBuf,
    /// Delete events older than this many days. Set to 0 to disable pruning.
    pub event_retention_days: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                data_dir: xdg_data_home().join("runsd"),
                socket_path: xdg_runtime_dir().join("runsd.sock"),
                port: 4242,
                max_concurrent_calculations: 8,
            },
            retry: RetryConfig {
                base_delay_ms: 1_000,
                max_delay_ms: 300_000,
                max_attempts: 5,
            },
            lease: LeaseConfig {
                heartbeat_interval_s: 10,
                expiry_s: 60,
                watchdog_interval_s: 30,
            },
            external_api: ExternalApiConfig {
                base_url: "https://example.com".to_string(),
                request_timeout_s: 30,
                supports_idempotency: true,
            },
            logging: LoggingConfig {
                file_level: "info".to_string(),
                stderr_level: "warn".to_string(),
                file_path: xdg_state_home().join("runsd/runsd.log"),
                event_retention_days: 30,
            },
        }
    }
}

impl Config {
    /// Load config: built-in defaults → config file → `RUNSD_*` env vars.
    #[allow(clippy::result_large_err)] // figment::Error is large; boxing would obscure the API
    pub fn load(config_file: Option<&std::path::Path>) -> figment::error::Result<Self> {
        let mut figment = Figment::from(Serialized::defaults(Config::default()));

        let default_path = xdg_config_home().join("runsd/config.toml");
        let path = config_file.unwrap_or(&default_path);
        if path.exists() {
            figment = figment.merge(Toml::file(path));
        }

        figment = figment.merge(Env::prefixed("RUNSD_").split("_"));
        figment.extract()
    }
}

// ── XDG Base Directory helpers ────────────────────────────────────────────────
//
// Spec: https://specifications.freedesktop.org/basedir-spec/latest/
//
// On Windows the XDG variables are not set; we map to the Windows equivalents
// (%APPDATA%, %LOCALAPPDATA%) so the same Config struct works everywhere.

/// `$XDG_DATA_HOME` — user-specific data files.
/// Default: `~/.local/share`  (Windows: `%APPDATA%`)
pub fn xdg_data_home() -> PathBuf {
    #[cfg(not(windows))]
    {
        std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home_dir().join(".local/share"))
    }
    #[cfg(windows)]
    {
        std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home_dir().join("AppData\\Roaming"))
    }
}

/// `$XDG_CONFIG_HOME` — user-specific config files.
/// Default: `~/.config`  (Windows: `%APPDATA%`)
pub fn xdg_config_home() -> PathBuf {
    #[cfg(not(windows))]
    {
        std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home_dir().join(".config"))
    }
    #[cfg(windows)]
    {
        std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home_dir().join("AppData\\Roaming"))
    }
}

/// `$XDG_STATE_HOME` — user-specific state files (logs, history).
/// Default: `~/.local/state`  (Windows: `%LOCALAPPDATA%`)
pub fn xdg_state_home() -> PathBuf {
    #[cfg(not(windows))]
    {
        std::env::var("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home_dir().join(".local/state"))
    }
    #[cfg(windows)]
    {
        std::env::var("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home_dir().join("AppData\\Local"))
    }
}

/// `$XDG_CACHE_HOME` — non-essential cached data.
/// Default: `~/.cache`  (Windows: `%LOCALAPPDATA%`)
pub fn xdg_cache_home() -> PathBuf {
    #[cfg(not(windows))]
    {
        std::env::var("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home_dir().join(".cache"))
    }
    #[cfg(windows)]
    {
        std::env::var("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home_dir().join("AppData\\Local"))
    }
}

/// `$XDG_RUNTIME_DIR` — runtime files (sockets, pipes). No Windows equivalent;
/// falls back to the OS temp dir which is session-scoped on all platforms.
pub fn xdg_runtime_dir() -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
}

/// Platform home directory.
fn home_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Ok(p) = std::env::var("USERPROFILE") {
            return PathBuf::from(p);
        }
        let drive = std::env::var("HOMEDRIVE").unwrap_or_default();
        let path  = std::env::var("HOMEPATH").unwrap_or_default();
        if !drive.is_empty() || !path.is_empty() {
            return PathBuf::from(format!("{drive}{path}"));
        }
        PathBuf::from("C:\\Users\\Default")
    }
    #[cfg(not(windows))]
    {
        std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp"))
    }
}
