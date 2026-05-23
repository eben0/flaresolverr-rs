use std::sync::Arc;
use dashmap::DashMap;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::browser::{BrowserSession, FetchRequest, PageResult};
use crate::error::{FlareSolverError, Result};

pub struct SessionStore {
    sessions: DashMap<String, Arc<Mutex<BrowserSession>>>,
    headless: bool,
    no_sandbox: bool,
    /// Pre-warmed browser for ephemeral (proxy-less) requests.
    /// Replaced on failure; None while starting up or after a crash.
    ephemeral: Mutex<Option<Arc<BrowserSession>>>,
}

impl SessionStore {
    pub fn new(headless: bool, no_sandbox: bool) -> Self {
        Self {
            sessions: DashMap::new(),
            headless,
            no_sandbox,
            ephemeral: Mutex::new(None),
        }
    }

    /// Start the shared ephemeral browser. Called once at server startup.
    /// Non-fatal: if Chrome fails to launch, ephemeral requests fall back to per-request browsers.
    pub async fn init_ephemeral(self: &Arc<Self>) {
        match BrowserSession::new(self.headless, self.no_sandbox, None).await {
            Ok(browser) => {
                *self.ephemeral.lock().await = Some(Arc::new(browser));
                tracing::info!("ephemeral browser ready");
            }
            Err(e) => {
                tracing::warn!("ephemeral browser failed to start (will use per-request browsers): {e}");
            }
        }
    }

    /// Fetch using the shared ephemeral browser (isolated context per request).
    /// Falls back to a fresh per-request browser if:
    ///   - a proxy is required (proxy is set at browser launch, can't be changed per-context)
    ///   - no pre-warmed browser is available yet (startup phase or after a crash)
    pub async fn ephemeral_fetch(self: &Arc<Self>, req: FetchRequest, proxy: Option<&str>) -> Result<PageResult> {
        if proxy.map(|p| !p.is_empty()).unwrap_or(false) {
            // Proxy requires a dedicated browser — browser context isolation can't change proxy.
            let session = BrowserSession::new(self.headless, self.no_sandbox, proxy).await?;
            return session.fetch(req).await;
        }

        let arc = { self.ephemeral.lock().await.clone() };
        if let Some(browser) = arc {
            let result = browser.fetch_isolated(req).await;
            if let Err(ref e) = result {
                tracing::warn!("shared ephemeral browser failed ({e}), restarting in background");
                *self.ephemeral.lock().await = None;
                let store = Arc::clone(self);
                tokio::spawn(async move { store.init_ephemeral().await });
            }
            return result;
        }

        // No pre-warmed browser available (startup phase or after a crash).
        let session = BrowserSession::new(self.headless, self.no_sandbox, None).await?;
        session.fetch(req).await
    }

    pub async fn create(&self, requested_id: Option<String>, proxy: Option<&str>) -> Result<String> {
        let id = requested_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        if self.sessions.contains_key(&id) {
            return Err(FlareSolverError::SessionAlreadyExists(id));
        }
        let session = BrowserSession::new(self.headless, self.no_sandbox, proxy).await?;
        self.sessions.insert(id.clone(), Arc::new(Mutex::new(session)));
        Ok(id)
    }

    pub fn list(&self) -> Vec<String> {
        self.sessions.iter().map(|e| e.key().clone()).collect()
    }

    pub async fn destroy(&self, session_id: &str) -> Result<()> {
        self.sessions
            .remove(session_id)
            .ok_or_else(|| FlareSolverError::SessionNotFound(session_id.to_string()))?;
        Ok(())
    }

    pub fn no_sandbox(&self) -> bool { self.no_sandbox }

    pub fn get(&self, session_id: &str) -> Option<Arc<Mutex<BrowserSession>>> {
        self.sessions.get(session_id).map(|e| Arc::clone(e.value()))
    }

    #[cfg(test)]
    pub fn destroy_sync(&self, session_id: &str) -> Result<()> {
        self.sessions
            .remove(session_id)
            .ok_or_else(|| FlareSolverError::SessionNotFound(session_id.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_empty() {
        let store = SessionStore::new(true, false);
        assert!(store.list().is_empty());
    }

    #[test]
    fn test_get_missing_returns_none() {
        let store = SessionStore::new(true, false);
        assert!(store.get("nonexistent").is_none());
    }

    #[test]
    fn test_destroy_missing_returns_error() {
        let store = SessionStore::new(true, false);
        let err = store.destroy_sync("nonexistent");
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("nonexistent"));
    }
}
