//! # sqlx-pg-test-template
//!
//! This crate provides a faster version of the `#[sqlx::test]` macro for PostgreSQL.
//! It creates a new database for every test using `CREATE DATABASE ... WITH TEMPLATE ...`
//! and drops it when the test completes.
//!
//! ## Overview
//!
//! Creating databases from a template is faster than running migrations for every test.
//! This tool is especially useful for integration tests that require a complex schema.
//!
//! ## Usage
//!
//! ```rust
//! use sqlx_pg_test_template::test;
//! use sqlx::{Pool, Postgres};
//!
//! // Basic usage using the database from DATABASE_URL as a template.
//! #[sqlx_pg_test_template::test]
//! async fn test_basic(pool: Pool<Postgres>) {
//!     // Test logic here
//! }
//!
//! // Use a specific template database.
//! #[sqlx_pg_test_template::test(template = "my_seed_template")]
//! async fn test_with_template(pool: Pool<Postgres>) {
//!     // Test logic here
//! }
//!
//! // Configure maximum pool connections for a specific test.
//! #[sqlx_pg_test_template::test(max_connections = 5)]
//! async fn test_with_custom_pool(pool: Pool<Postgres>) {
//!     // Test logic here
//! }
//! ```
//!
//! ## Running Tests
//!
//! Set the `DATABASE_URL` to point to your template database:
//!
//! ```sh
//! DATABASE_URL="postgres://user:pass@localhost:5432/template_db" cargo test
//! ```
//!
//! The runner uses the connection to the default system database (usually `postgres`)
//! to manage test databases. Ensure the user has permissions to create and drop databases.
//!
//! ## Requirements
//!
//! - The PostgreSQL user must have `CREATEDB` permissions.
//! - A default database (e.g., `postgres`) must be accessible to the user.
//! - The template database must exist and be up-to-date.
//!
//! ## Differences from `#[sqlx::test]`
//!
//! - **Speed**: Uses PostgreSQL templates instead of re-running migrations.
//! - **Concurrency**: Optimized for `nextest` by avoiding shared state between test processes.
//! - **Naming**: Uses a hash of the test module path for database names to avoid collisions.
//!
//! ## Maintenance
//!
//! You must update the template database manually when your schema changes.
//! For example:
//!
//! ```sh
//! sqlx database create --database-url $DATABASE_URL
//! sqlx migrate run --database-url $DATABASE_URL
//! ```

pub use sqlx_pg_test_template_macros::test;

#[doc(hidden)]
pub use sqlx_pg_test_template_runner::TestArgs;

#[doc(hidden)]
pub use sqlx_pg_test_template_runner::run_test;
