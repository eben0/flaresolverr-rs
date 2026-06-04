use chaser_cf::ChaserConfig;
use serde::Deserialize;

/// Library-facing configuration for the CF bypass engine.
/// Construct with `FlareSolverConfig::default()` and override fields as needed.
#[derive(Debug, Clone, Deserialize)]
pub struct FlareSolverConfig {
    #[serde(default = "default_headless")]
    pub headless: bool,
    #[serde(default)]
    pub virtual_display: bool,
    #[serde(default)]
    pub lazy_init: bool,
    #[serde(default = "default_max_timeout")]
    pub max_timeout_ms: u64,
    #[serde(default = "default_context_limit")]
    pub context_limit: usize,
    #[serde(default = "default_no_sandbox")]
    pub no_sandbox: bool,
}

impl Default for FlareSolverConfig {
    fn default() -> Self {
        Self {
            headless: true,
            virtual_display: false,
            lazy_init: false,
            max_timeout_ms: 60_000,
            context_limit: 20,
            no_sandbox: true,
        }
    }
}

impl FlareSolverConfig {
    pub fn to_chaser_config(&self) -> ChaserConfig {
        let mut cfg = ChaserConfig::default()
            .with_context_limit(self.context_limit)
            .with_timeout_ms(self.max_timeout_ms)
            .with_headless(self.headless)
            .with_virtual_display(self.virtual_display)
            .with_lazy_init(self.lazy_init);
        if self.no_sandbox {
            cfg = cfg.add_extra_arg("--no-sandbox");
        }
        cfg
    }
}

fn default_headless() -> bool { true }
fn default_max_timeout() -> u64 { 60_000 }
fn default_context_limit() -> usize { 20 }
fn default_no_sandbox() -> bool { true }

// ─── Server-only config ──────────────────────────────────────────────────────

#[cfg(feature = "server")]
pub use server::ServerConfig;
#[cfg(feature = "server")]
pub use server::load;

#[cfg(feature = "server")]
mod server {
    use figment::{
        providers::{Env, Format, Toml},
        Figment,
    };
    use serde::Deserialize;
    use super::FlareSolverConfig;

    #[derive(Debug, Clone, Deserialize)]
    pub struct ServerConfig {
        #[serde(default = "default_host")]
        pub host: String,
        #[serde(default = "default_port")]
        pub port: u16,
        #[serde(default = "default_log_level")]
        pub log_level: String,
    }

    impl Default for ServerConfig {
        fn default() -> Self {
            Self {
                host: "0.0.0.0".into(),
                port: 8191,
                log_level: "info".into(),
            }
        }
    }

    /// Load configuration by merging `config.toml` and `FLARESOLVERR_` env vars.
    pub fn load() -> Result<(FlareSolverConfig, ServerConfig), Box<figment::Error>> {
        let solver: FlareSolverConfig = Figment::new()
            .merge(Toml::file("config.toml"))
            .merge(Env::prefixed("FLARESOLVERR_"))
            .extract()
            .map_err(Box::new)?;
        let server: ServerConfig = Figment::new()
            .merge(Toml::file("config.toml"))
            .merge(Env::prefixed("FLARESOLVERR_"))
            .extract()
            .map_err(Box::new)?;
        Ok((solver, server))
    }

    fn default_host() -> String { "0.0.0.0".into() }
    fn default_port() -> u16 { 8191 }
    fn default_log_level() -> String { "info".into() }
}
