use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
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
}

fn default_host() -> String { "0.0.0.0".into() }
fn default_port() -> u16 { 8191 }
fn default_max_timeout() -> u64 { 60_000 }
fn default_headless() -> bool { true }
fn default_log_level() -> String { "info".into() }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defaults() {
        let cfg: FlareSolverConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.host, "0.0.0.0");
        assert_eq!(cfg.port, 8191);
        assert_eq!(cfg.max_timeout_ms, 60_000);
        assert!(cfg.headless);
        assert_eq!(cfg.log_level, "info");
    }

    #[test]
    fn test_override() {
        let cfg: FlareSolverConfig =
            serde_json::from_str(r#"{"port":9000,"headless":false}"#).unwrap();
        assert_eq!(cfg.port, 9000);
        assert!(!cfg.headless);
        assert_eq!(cfg.host, "0.0.0.0");
    }
}
