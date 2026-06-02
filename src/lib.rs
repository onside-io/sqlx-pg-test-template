// File: src/lib.rs
// Purpose: Public API for the sqlx-pg-test-template crate.

//! # sqlx-pg-test-template
//!
//! Provides a fast PostgreSQL test runner for `sqlx`.
//! It uses database templates for rapid isolation between tests.
//!
//! ## Overview
//!
//! This crate simplifies integration testing by cloning a template database
//! for each test. This is faster than running migrations for every test case.
//!
//! ## Usage
//!
//! ```rust
//! use sqlx_pg_test_template::test;
//! use sqlx::{Pool, Postgres};
//!
//! #[sqlx_pg_test_template::test]
//! async fn test_basic(pool: Pool<Postgres>) {
//!     // ...
//! }
//!
//! // Use a specific template database.
//! #[sqlx_pg_test_template::test(template = "my_seed_template")]
//! async fn test_with_template(pool: Pool<Postgres>) {
//!     // ...
//! }
//!
//! // Configure maximum pool connections for a specific test.
//! #[sqlx_pg_test_template::test(max_connections = 5)]
//! async fn test_with_custom_pool(pool: Pool<Postgres>) {
//!     // ...
//! }
//!
//! // Configure to keep the database if the test fails (useful for debugging).
//! #[sqlx_pg_test_template::test(keep_db_on_failure = true)]
//! async fn test_keep_db_on_failure(pool: Pool<Postgres>) {
//!     // ...
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
