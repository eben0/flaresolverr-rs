use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use chaser_cf::ChaserCF;
use dashmap::DashMap;
use uuid::Uuid;

use crate::error::{FlareSolverError, Result};

pub struct SessionEntry {
    pub proxy_url: Option<String>,
    pub created_at: u64,
}

/// Pure session registry — no browser dependency. Fully testable synchronously.
pub struct SessionRegistry {
    sessions: DashMap<String, SessionEntry>,
}

impl SessionRegistry {
    pub fn new() -> Self {
        Self {
            sessions: DashMap::new(),
        }
    }

    pub fn create(&self, id: Option<String>, proxy_url: Option<&str>) -> Result<String> {
        let id = id.unwrap_or_else(|| Uuid::new_v4().to_string());
        if self.sessions.contains_key(&id) {
            return Err(FlareSolverError::SessionAlreadyExists(id));
        }
        self.sessions.insert(
            id.clone(),
            SessionEntry {
                proxy_url: proxy_url.map(|s| s.to_string()),
                created_at: unix_ms(),
            },
        );
        Ok(id)
    }

    pub fn list(&self) -> Vec<String> {
        self.sessions.iter().map(|e| e.key().clone()).collect()
    }

    pub fn destroy(&self, id: &str) -> Result<()> {
        self.sessions
            .remove(id)
            .ok_or_else(|| FlareSolverError::SessionNotFound(id.to_string()))?;
        Ok(())
    }

    pub fn get_proxy(&self, id: &str) -> Option<String> {
        self.sessions.get(id).and_then(|e| e.proxy_url.clone())
    }

    pub fn contains(&self, id: &str) -> bool {
        self.sessions.contains_key(id)
    }
}

impl Default for SessionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Application-level session store: registry + shared ChaserCF instance + shared HTTP client.
pub struct SessionStore {
    pub registry: SessionRegistry,
    pub chaser: Arc<ChaserCF>,
    /// Shared reqwest client for no-proxy requests — reuses TCP connections and TLS sessions.
    pub client: reqwest::Client,
}

impl SessionStore {
    pub fn new(chaser: Arc<ChaserCF>) -> Self {
        Self {
            registry: SessionRegistry::new(),
            chaser,
            client: reqwest::Client::builder()
                .user_agent(crate::solver::DEFAULT_UA)
                .build()
                .expect("failed to build shared HTTP client"),
        }
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
