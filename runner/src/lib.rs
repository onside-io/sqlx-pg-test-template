// File: runner/src/lib.rs
// Purpose: Runtime logic for PostgreSQL test database management.

//! # sqlx-pg-test-template-runner
//!
//! Provides the runtime logic to create, manage, and delete temporary
//! PostgreSQL databases for isolation during tests.

use futures_util::FutureExt;
use std::hash::Hasher;
use std::str::FromStr;

use sqlx::{
    Connection, PgConnection, Pool, Postgres,
    postgres::{PgConnectOptions, PgPoolOptions},
};

/// Errors encountered during test database management.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `DATABASE_URL` is not set or its value is invalid.
    #[error("DATABASE_URL is missing or invalid")]
    InvalidDatabaseUrl,

    /// Database name cannot be determined from connection options.
    #[error("database not found for an open connection pool")]
    DatabaseNotFound,

    /// An error occurred during a `sqlx` operation.
    #[error("sqlx error: '{0}'")]
    Sqlx(#[from] sqlx::Error),
}

/// Configuration for a specific test execution.
pub struct TestArgs {
    /// Name of the PostgreSQL database to use as a template.
    pub template_name: Option<String>,

    /// Maximum concurrent connections in the test connection pool.
    pub max_connections: Option<u32>,

    /// Path of the test module. Used to generate unique database names.
    pub module_path: String,

    /// If true, the database is not dropped if the test fails.
    pub keep_db_on_failure: bool,
}

/// Creates a unique PostgreSQL database by cloning a template database.
///
/// # Arguments
///
/// * `conn` - Active connection to the PostgreSQL server.
/// * `template_db_name` - Name of the template database to clone.
/// * `module_path` - Path of the test, used for unique name generation.
///
/// # Returns
///
/// Returns a tuple containing the new database name and the connection, or an `Error`.
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

/// Initializes a connection pool for a specific test database.
///
/// # Arguments
///
/// * `connect_options` - Connection parameters for the server.
/// * `db_name` - Name of the database to connect to.
/// * `max_connections` - Optional limit on the number of pool connections.
///
/// # Returns
///
/// Returns a configured `Pool<Postgres>` or an `Error`.
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

/// Extracts the database name from connection options.
///
/// # Arguments
///
/// * `connect_opts` - PostgreSQL connection options.
///
/// # Returns
///
/// Returns the database name as a `String` or `Error::DatabaseNotFound`.
pub fn db_name_of_test_pool(connect_opts: &PgConnectOptions) -> Result<String, Error> {
    connect_opts
        .get_database()
        .map(|s| s.to_string())
        .ok_or(Error::DatabaseNotFound)
}

/// Closes the connection pool and deletes the temporary test database.
///
/// This function uses `DROP DATABASE ... WITH (FORCE)` to ensure cleanup.
///
/// # Arguments
///
/// * `conn` - Active connection to the PostgreSQL server.
/// * `pool` - The connection pool for the database to be dropped.
///
/// # Returns
///
/// Returns `Ok(())` on success or an `Error`.
pub async fn close_test_pool(conn: &mut PgConnection, pool: &Pool<Postgres>) -> Result<(), Error> {
    let db_name = db_name_of_test_pool(&pool.connect_options())?;

    pool.close().await;

    sqlx::query(&format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", db_name))
        .execute(conn)
        .await?;

    Ok(())
}

/// Manages the full lifecycle of a database-backed test.
///
/// This function handles setup, execution, and cleanup of the test database.
///
/// The test function may accept either a `Pool<Postgres>` or any type `P` that
/// implements `From<Pool<Postgres>>`. This allows client code to use a newtype
/// wrapper around the pool (for example an application-specific `Db` handle)
/// without giving up the managed database lifecycle. The runner retains its own
/// `Pool<Postgres>` for cleanup, so wrapping never interferes with teardown.
///
/// # Arguments
///
/// * `f` - The asynchronous test function to execute.
/// * `args` - Test configuration parameters.
///
/// # Returns
///
/// Returns `Ok(())` if the lifecycle completes successfully.
pub async fn wrap_run_test<F, Fut, P>(f: F, args: TestArgs) -> Result<(), Error>
where
    P: From<Pool<Postgres>>,
    F: Fn(P) -> Fut,
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
    let test_result = std::panic::AssertUnwindSafe(f(pool.clone().into()))
        .catch_unwind()
        .await;

    // Close the pool & drop the test database
    let mut conn = PgConnection::connect_with(&service_connect_opts).await?;
    let cleanup_result = if !args.keep_db_on_failure || test_result.is_ok() {
        close_test_pool(&mut conn, &pool).await
    } else {
        pool.close().await;
        Ok(())
    };
    let conn_close_result =  conn.close().await;

    if let Err(err) = test_result {
        // Don't return the test error as a Result, but instead panic to fail the test.
        // This ensures that the test failure is properly reported by the test framework.
        std::panic::resume_unwind(err);
    }

    cleanup_result?;
    conn_close_result?;

    Ok(())
}

/// Synchronous wrapper that executes a test lifecycle within a runtime block.
///
/// The test function may accept either a `Pool<Postgres>` or any type `P` that
/// implements `From<Pool<Postgres>>`.
///
/// # Arguments
///
/// * `f` - The asynchronous test function to execute.
/// * `args` - Test configuration parameters.
pub fn run_test<F, Fut, P>(f: F, args: TestArgs)
where
    P: From<Pool<Postgres>>,
    F: Fn(P) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    sqlx::test_block_on(async move {
        match wrap_run_test(f, args).await {
            Err(e) => panic!("test failed: {e}"),
            Ok(v) => v,
        }
    })
}
