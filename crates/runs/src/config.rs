use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunsConfig {
    /// Unix socket path (Unix/macOS). Overridden by --socket / RUNSD_SOCKET.
    #[cfg(unix)]
    pub socket_path: Option<PathBuf>,
    /// TCP port for the runsd server (Windows). Overridden by --port / RUNSD_PORT.
    #[cfg(windows)]
    pub port: u16,
    /// Number of runs to fetch per page.
    pub page_size: u32,
}

impl Default for RunsConfig {
    fn default() -> Self {
        Self {
            #[cfg(unix)]
            socket_path: None,
            #[cfg(windows)]
            port: 4242,
            page_size: 100,
        }
    }
}

impl RunsConfig {
    /// Load: built-in defaults → `~/.config/runs/config.toml` → `RUNS_*` env vars.
    pub fn load() -> Self {
        let path = config_path();
        let mut f = Figment::from(Serialized::defaults(RunsConfig::default()));
        if path.exists() {
            f = f.merge(Toml::file(&path));
        }
        f = f.merge(Env::prefixed("RUNS_").split("_"));
        f.extract().unwrap_or_default()
    }
}

fn config_path() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(|p| PathBuf::from(p).join("runs/config.toml"))
        .unwrap_or_else(|_| {
            #[cfg(not(windows))]
            {
                std::env::var("HOME")
                    .map(|h| PathBuf::from(h).join(".config/runs/config.toml"))
                    .unwrap_or_else(|_| PathBuf::from("/tmp/runs-config.toml"))
            }
            #[cfg(windows)]
            {
                std::env::var("APPDATA")
                    .map(|p| PathBuf::from(p).join("runs\\config.toml"))
                    .unwrap_or_else(|_| PathBuf::from("runs-config.toml"))
            }
        })
}
