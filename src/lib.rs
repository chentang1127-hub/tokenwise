//! TokenWise Core library — re-exports modules for both the binary and
//! integration tests.
//!
//! Architecture:
//!   Your App ──→ Proxy (:9401) ──→ Smart Router ──→ AI APIs
//!                       │
//!                       └── Admin Dashboard (:9400)

pub mod admin;
pub mod cache;
pub mod config;
pub mod cost;
pub mod grpc_proxy;
pub mod import;
pub mod license;
pub mod multi_user;
pub mod proxy;
pub mod recording;
pub mod webhooks;
