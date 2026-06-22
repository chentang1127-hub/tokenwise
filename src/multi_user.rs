//! Multi-user / multi-tenant support foundation.
//!
//! Each API key identifies a tenant. Calls are tagged with a `tenant_id`
//! derived from a SHA-256 hash of the API key (zero-trust — we never
//! store the raw key, only its hash).
//!
//! Dashboard data, budget tracking, and cache entries are all scoped
//! to a tenant. The admin API uses a `?tenant=...` query parameter or
//! `X-TokenWise-Tenant` header to filter data.

use sha2::{Digest, Sha256};

/// A tenant identifier — SHA-256 of the user's API key.
/// We never store the raw key, only this hash.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TenantId(pub String);

impl TenantId {
    /// Derive a tenant ID from an API key.
    pub fn from_api_key(key: &str) -> Self {
        // Strip "Bearer " prefix if present
        let key = key.strip_prefix("Bearer ").unwrap_or(key);
        let mut hasher = Sha256::new();
        hasher.update(key.as_bytes());
        let hash = hasher.finalize();
        Self(hex::encode(hash))
    }

    /// Anonymous tenant (no API key provided).
    pub fn anonymous() -> Self {
        Self("anon".to_string())
    }
}

impl std::fmt::Display for TenantId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Show only first 8 chars for display
        if self.0 == "anon" {
            write!(f, "anon")
        } else {
            write!(f, "{}...", &self.0[..8.min(self.0.len())])
        }
    }
}

/// Tenant-aware request context.
#[derive(Debug, Clone)]
pub struct TenantContext {
    pub tenant_id: TenantId,
    /// Whether this tenant has Pro features enabled.
    pub is_pro: bool,
}

/// Tenant-scoped budget tracker.
/// Each tenant gets their own budget counters.
#[derive(Debug, Clone, Default)]
pub struct TenantBudget {
    /// Total cost in USD for the current day (tenant-scoped).
    pub spent_today: f64,
    /// Total cost this month.
    pub spent_month: f64,
    /// Cached timestamp of when these counters were computed.
    pub cached_at: i64,
}

/// Extract tenant ID from an HTTP Authorization header.
pub fn extract_tenant(auth_header: Option<&str>) -> TenantId {
    match auth_header {
        Some(h) if !h.is_empty() => TenantId::from_api_key(h),
        _ => TenantId::anonymous(),
    }
}
