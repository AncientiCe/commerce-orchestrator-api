//! REST API service for the commerce orchestrator.

pub mod app;
pub mod auth;
pub mod config;
pub mod dto;
pub mod error;
pub mod observability;
pub mod routes;
pub mod state;

pub use app::{app, serve};
pub use auth::{AuthContextExtractor, OptionalAuthContext, StaticTokenAuthnResolver};
pub use config::{
    default_config_path, ComponentsConfig, EnvProfile, HttpClientConfig, ProductionConfig,
    ResolvedComponents, ServerConfig,
};
pub use error::ApiError;
pub use state::AppState;
