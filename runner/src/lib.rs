//! # sqlx-pg-test-template-runner
//!
//! This module contains the runtime logic for creating, managing, and cleaning up
//! temporary PostgreSQL databases for tests.

use futures_util::FutureExt;
use std::hash::Hasher;
use std::str::FromStr;

use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    Connection, PgConnection, Pool, Postgres,
};

/// Errors encountered during test database management.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The `DATABASE_URL` environment variable is not set or its value is invalid.
    #[error("DATABASE_URL is missing or invalid")]
    InvalidDatabaseUrl,

    /// Could not determine the database name from the connection options.
    #[error("database not found for an open connection pool")]
    DatabaseNotFound,

    /// An error occurred while executing a SQL command via `sqlx`.
    #[error("sqlx error: '{0}'")]
    Sqlx(#[from] sqlx::Error),
}

/// Arguments required to initialize a specific test.
pub struct TestArgs {
    /// The name of the PostgreSQL database to use as a template.
    pub template_name: Option<String>,

    /// The maximum number of concurrent connections for the test pool.
    pub max_connections: Option<u32>,

    /// The unique module path of the test, used to generate a unique database name.
    pub module_path: String,
}

/// Creates a new PostgreSQL database using another database as a template.
///
/// # Arguments
///
/// * `conn` - An active connection to the PostgreSQL server (typically to the `postgres` database).
/// * `template_db_name` - The name of the database to clone.
/// * `module_path` - The path to the test function, used to ensure database name uniqueness via hashing.
///
/// # Returns
///
/// Returns a tuple containing the new database name and the updated connection, or an `Error`.
pub async fn create_db_from_template(
    mut conn: PgConnection,
    template_db_name: &str,
    module_path: &str,
) -> Result<(String, PgConnection), Error> {
    let mut hasher = std::hash::DefaultHasher::new();
    hasher.write(module_path.as_bytes());
    let id = hasher.finish();

    // Use a unique name based on the test path to allow parallel test execution.
    let db_name = format!("_sqlx_{}", id);

    sqlx::query(&format!("DROP DATABASE IF EXISTS {}", db_name))
        .execute(&mut conn)
        .await?;

    sqlx::query(&format!(
        "CREATE DATABASE {} WITH TEMPLATE {}",
        db_name, template_db_name
    ))
    .execute(&mut conn)
    .await?;

    // Store the module path in a comment to help identify leaked databases.
    sqlx::query(&format!(
        "COMMENT ON DATABASE {} IS '{}'",
        db_name, module_path
    ))
    .execute(&mut conn)
    .await?;

    Ok((db_name, conn))
}

/// Initializes a connection pool for the newly created test database.
///
/// # Arguments
///
/// * `connect_options` - Base connection options (e.g., host, user).
/// * `db_name` - The name of the specific test database to connect to.
/// * `max_connections` - Optional limit on the number of pool connections.
pub async fn spawn_test_pool(
    connect_options: &PgConnectOptions,
    db_name: &str,
    max_connections: Option<u32>,
) -> Result<Pool<Postgres>, Error> {
    let connect_options = connect_options.clone().database(db_name);
    let pool = PgPoolOptions::new()
        .max_connections(max_connections.unwrap_or(2))
        .idle_timeout(Some(std::time::Duration::from_secs(1)))
        .connect_with(connect_options)
        .await?;

    Ok(pool)
}

/// Extracts the database name from PostgreSQL connection options.
///
/// # Arguments
///
/// * `connect_options` - Base connection options (e.g., host, user).
///
/// # Returns
///
/// Returns the database name as a `String` on success, or an `Error::DatabaseNotFound`.
pub fn db_name_of_test_pool(connect_opts: &PgConnectOptions) -> Result<String, Error> {
    connect_opts
        .get_database()
        .map(|s| s.to_string())
        .ok_or(Error::DatabaseNotFound)
}

/// Closes the test pool and deletes the temporary test database.
///
/// This function forces the database drop even if there are active connections.
///
/// # Arguments
///
/// * `conn` - An active connection to the PostgreSQL server (typically to the `postgres` database).
/// * `pool` - The connection pool associated with the test database to be dropped.
pub async fn close_test_pool(conn: &mut PgConnection, pool: &Pool<Postgres>) -> Result<(), Error> {
    let db_name = db_name_of_test_pool(&pool.connect_options())?;

    pool.close().await;

    sqlx::query(&format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", db_name))
        .execute(conn)
        .await?;

    Ok(())
}

/// Orchestrates the execution of a single test.
///
/// This function:
/// 1. Connects to the server.
/// 2. Creates a unique database from a template.
/// 3. Runs the test closure with a dedicated connection pool.
/// 4. Cleans up the database after the test finishes (or fails).
///
/// # Arguments
///
/// * `f` - The test closure or function to execute.
/// * `args` - Configuration parameters for the test, such as template name and module path.
pub async fn wrap_run_test<F, Fut>(f: F, args: TestArgs) -> Result<(), Error>
where
    F: Fn(Pool<Postgres>) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    // Get connection string
    let database_url = std::env::var("DATABASE_URL").map_err(|_| Error::InvalidDatabaseUrl)?;

    // Try to get template database name from args, defaulting to connection database name
    let connect_opts = PgConnectOptions::from_str(&database_url)?;

    // Get template name from args or use database name from connection options
    let template_name = &args
        .template_name
        .map(Ok)
        .unwrap_or_else(|| db_name_of_test_pool(&connect_opts))?;

    // Service connection == default database (postgres, username, etc.)
    let service_connect_opts = connect_opts.clone().database("");

    // Create a new database from the template
    let conn = PgConnection::connect_with(&service_connect_opts).await?;
    let (db_name, conn) = create_db_from_template(conn, template_name, &args.module_path).await?;
    conn.close().await?;

    // Run test
    let pool = spawn_test_pool(&service_connect_opts, &db_name, args.max_connections).await?;
    let test_result = std::panic::AssertUnwindSafe(f(pool.clone()))
        .catch_unwind()
        .await;

    // Close the pool & drop the test database
    let mut conn = PgConnection::connect_with(&service_connect_opts).await?;
    let cleanup_result = close_test_pool(&mut conn, &pool).await;
    conn.close().await?;

    if let Err(err) = test_result {
        // Don't return the test error as a Result, but instead panic to fail the test.
        // This ensures that the test failure is properly reported by the test framework.
        std::panic::resume_unwind(err);
    }

    cleanup_result?;

    Ok(())
}

/// A synchronous wrapper for `wrap_run_test` that uses `sqlx::test_block_on`.
///
/// # Arguments
///
/// * `f` - The test closure or function to execute.
/// * `args` - Configuration parameters for the test environment.
pub fn run_test<F, Fut>(f: F, args: TestArgs)
where
    F: Fn(Pool<Postgres>) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    sqlx::test_block_on(async move {
        match wrap_run_test(f, args).await {
            Err(e) => panic!("test failed: {e}"),
            Ok(v) => v,
        }
    })
}
