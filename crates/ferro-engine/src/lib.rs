//! Ferro engine: database backends (PostgreSQL primary), storage, items, auth, sync.

pub mod activity;
pub mod assistant;
pub mod aggregate;
pub mod auth;
pub mod db;
pub mod ddl;
pub mod error;
pub mod files;
pub mod flows;
pub mod graphql;
pub mod images;
pub mod items;
pub mod permissions;
pub mod query;
pub mod realtime;
pub mod schema;
pub mod schema_store;
pub mod sync;
pub mod tfa;
pub mod translations;
pub mod versioning;
pub mod workspace;

pub use error::{FerroError, Result};
pub use permissions::Accountability;
pub use schema::{Schema, SchemaHolder};
