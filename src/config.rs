use chaser_cf::ChaserConfig;
use figment::{
    providers::{Env, Format, Toml},
    Figment,
};
use serde::Deserialize;

/// Load configuration by merging `config.toml` and `FLARESOLVERR_*` env vars.
pub fn load() -> Result<FlareSolverConfig, Box<figment::Error>> {
    Figment::new()
        .merge(Toml::file("config.toml"))
        .merge(Env::prefixed("FLARESOLVERR_"))
        .extract()
        .map_err(Box::new)
}

/// Top-level FlareSolverr configuration.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FlareSolverConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_max_timeout")]
    pub max_timeout_ms: u64,
    #[serde(default = "default_headless")]
    pub headless: bool,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_no_sandbox")]
    pub no_sandbox: bool,
    #[serde(default = "default_context_limit")]
    pub context_limit: usize,
    #[serde(default)]
    pub virtual_display: bool,
}

impl FlareSolverConfig {
    /// Build a [`ChaserConfig`] from this configuration.
    pub fn to_chaser_config(&self) -> ChaserConfig {
        let mut cfg = ChaserConfig::default()
            .with_context_limit(self.context_limit)
            .with_timeout_ms(self.max_timeout_ms)
            .with_headless(self.headless)
            .with_virtual_display(self.virtual_display);
        if self.no_sandbox {
            cfg = cfg.add_extra_arg("--no-sandbox");
        }
        cfg
    }
}

fn default_host() -> String {
    "0.0.0.0".into()
}
fn default_port() -> u16 {
    8191
}
fn default_max_timeout() -> u64 {
    60_000
}
fn default_headless() -> bool {
    true
}
fn default_log_level() -> String {
    "info".into()
}
fn default_no_sandbox() -> bool {
    true
}
fn default_context_limit() -> usize {
    20
}
