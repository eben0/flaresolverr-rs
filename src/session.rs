use std::sync::Arc;
use dashmap::DashMap;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::browser::BrowserSession;
use crate::error::{FlareSolverError, Result};

pub struct SessionStore {
    sessions: DashMap<String, Arc<Mutex<BrowserSession>>>,
    headless: bool,
}

impl SessionStore {
    pub fn new(headless: bool) -> Self {
        Self { sessions: DashMap::new(), headless }
    }

    pub async fn create(&self, requested_id: Option<String>, proxy: Option<&str>) -> Result<String> {
        let id = requested_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        if self.sessions.contains_key(&id) {
            return Ok(id);
        }
        let session = BrowserSession::new(self.headless, proxy).await?;
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
        let store = SessionStore::new(true);
        assert!(store.list().is_empty());
    }

    #[test]
    fn test_get_missing_returns_none() {
        let store = SessionStore::new(true);
        assert!(store.get("nonexistent").is_none());
    }

    #[test]
    fn test_destroy_missing_returns_error() {
        let store = SessionStore::new(true);
        let err = store.destroy_sync("nonexistent");
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("nonexistent"));
    }
}
