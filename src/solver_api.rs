use std::sync::Arc;

use crate::config::FlareSolverConfig;
use crate::error::Result;
use crate::models::{FetchRequest, FetchResponse};
use crate::session::SessionStore;
use crate::solver;

/// High-level CF bypass client. Create with [`FlareSolver::new`], then call
/// [`get`](FlareSolver::get), [`post`](FlareSolver::post), or [`fetch`](FlareSolver::fetch).
pub struct FlareSolver {
    store: Arc<SessionStore>,
    max_timeout_ms: u64,
}

impl FlareSolver {
    /// Initialize the browser engine and return a ready solver.
    pub async fn new(config: FlareSolverConfig) -> Result<Self> {
        let max_timeout_ms = config.max_timeout_ms;
        let lazy = config.lazy_init;
        let store = Arc::new(SessionStore::new(config.to_chaser_config()));
        // Launch Chrome eagerly (fail fast) unless the caller opted into lazy init.
        if !lazy {
            store.browser().await?;
        }
        Ok(Self {
            store,
            max_timeout_ms,
        })
    }

    /// Fetch a URL via GET.
    pub async fn get(&self, url: &str) -> Result<FetchResponse> {
        self.fetch(FetchRequest::get(url)).await
    }

    /// Fetch a URL via POST with a URL-encoded body.
    pub async fn post(&self, url: &str, body: &str) -> Result<FetchResponse> {
        self.fetch(FetchRequest::post(url).body(body)).await
    }

    /// Fetch a URL with full control over proxy, cookies, and method.
    pub async fn fetch(&self, req: FetchRequest) -> Result<FetchResponse> {
        let browser = self.store.browser().await?;
        solver::fetch(
            browser,
            &req.url,
            req.is_post,
            req.post_data.as_deref(),
            req.proxy.as_deref(),
            &req.cookies,
            self.max_timeout_ms,
        )
        .await
    }

    /// Expose the underlying session store for HTTP server integration.
    #[cfg(feature = "server")]
    pub fn session_store(&self) -> Arc<SessionStore> {
        Arc::clone(&self.store)
    }
}
