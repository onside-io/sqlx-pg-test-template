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

    /// PostgreSQL refused to clone the template because other sessions are connected to it.
    ///
    /// The runner's own maintenance connections use the role's default database, so the
    /// template named in `DATABASE_URL` must be a different database that nothing connects to.
    #[error(
        "cannot clone template database `{template}`: {source}. PostgreSQL refuses to copy a \
         template while other sessions are connected to it; point DATABASE_URL at a dedicated \
         template database that nothing connects to (the runner's own maintenance connections \
         use the role's default database)"
    )]
    TemplateInUse {
        /// Name of the template database that could not be cloned.
        template: String,
        /// The underlying database error.
        #[source]
        source: sqlx::Error,
    },

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

    /// If true, the database is not dropped if the test fails (panics or returns `Err`).
    pub keep_db_on_failure: bool,
}

/// Return types a test function may have.
///
/// A test that ran to completion (did not panic) succeeded if its return value says so. The
/// runner uses this to decide whether a failed test keeps its database (`keep_db_on_failure`).
///
/// Implemented for `()` (always a success) and `Result<T, E>` (a success if `Ok`), so a test can
/// be written as `async fn t(pool: Pool<Postgres>) -> Result<(), E>` and use `?` in its body. The
/// test framework additionally requires the return type of the generated `#[test]` function to
/// implement [`std::process::Termination`], which for `Result<(), E>` means `E: Debug`.
pub trait TestTermination {
    /// Returns `true` if the value represents a successful test.
    fn is_success(&self) -> bool;
}

impl TestTermination for () {
    fn is_success(&self) -> bool {
        true
    }
}

impl<T, E> TestTermination for Result<T, E> {
    fn is_success(&self) -> bool {
        self.is_ok()
    }
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
    .await
    .map_err(|err| template_error(err, template_db_name))?;

    // Store the module path in a comment to help identify leaked databases.
    sqlx::query(&format!(
        "COMMENT ON DATABASE {} IS '{}'",
        db_name, module_path
    ))
    .execute(&mut conn)
    .await?;

    Ok((db_name, conn))
}

/// SQLSTATE reported by PostgreSQL when a template database has other active sessions.
const OBJECT_IN_USE: &str = "55006";

/// Converts a failed `CREATE DATABASE ... WITH TEMPLATE` into the most descriptive error.
///
/// # Arguments
///
/// * `err` - The error returned by the `CREATE DATABASE` statement.
/// * `template` - Name of the template database that was being cloned.
fn template_error(err: sqlx::Error, template: &str) -> Error {
    let template_in_use = matches!(
        &err,
        sqlx::Error::Database(db_err) if db_err.code().as_deref() == Some(OBJECT_IN_USE)
    );

    if template_in_use {
        Error::TemplateInUse {
            template: template.to_owned(),
            source: err,
        }
    } else {
        Error::Sqlx(err)
    }
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

/// Closes the pool and, if `drop_db` is set, drops the test database.
///
/// Every step runs even when an earlier one fails, so that neither the pool nor the
/// maintenance connection is leaked; the first error is returned.
///
/// # Arguments
///
/// * `service_connect_opts` - Connection options for the role's default database.
/// * `pool` - The connection pool of the test database.
/// * `drop_db` - Whether to drop the test database after closing the pool.
async fn cleanup_test_db(
    service_connect_opts: &PgConnectOptions,
    pool: &Pool<Postgres>,
    drop_db: bool,
) -> Result<(), Error> {
    let mut conn = match PgConnection::connect_with(service_connect_opts).await {
        Ok(conn) => conn,
        Err(err) => {
            pool.close().await;
            return Err(err.into());
        }
    };
    let drop_result = if drop_db {
        close_test_pool(&mut conn, pool).await
    } else {
        pool.close().await;
        Ok(())
    };
    let conn_close_result = conn.close().await;

    drop_result?;
    conn_close_result?;

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
/// The test function may return `()` or a `Result` (see [`TestTermination`]). A panic and a
/// returned `Err` both count as a failed test, which matters for `keep_db_on_failure`.
///
/// # Arguments
///
/// * `f` - The asynchronous test function to execute.
/// * `args` - Test configuration parameters.
///
/// # Returns
///
/// Returns `Ok` with the test's own return value once the test ran, whether it succeeded or
/// not, and `Err` if the runner failed to set up the database or to clean up after a
/// successful test.
pub async fn wrap_run_test<F, Fut, P>(f: F, args: TestArgs) -> Result<Fut::Output, Error>
where
    P: From<Pool<Postgres>>,
    F: Fn(P) -> Fut,
    Fut: std::future::Future,
    Fut::Output: TestTermination,
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

    // Build the caught future first and perform the `P: From<Pool<Postgres>>`
    // conversion (and the call to `f`) inside it. Both are user-supplied code that
    // may panic; doing them lazily inside `catch_unwind` ensures such a panic is
    // captured here so the cleanup below still runs and the temporary database is
    // not leaked.
    let test_pool = pool.clone();
    let test_result = std::panic::AssertUnwindSafe(async move { f(test_pool.into()).await })
        .catch_unwind()
        .await;

    // A panic and a returned `Err` both count as a failed test.
    let test_succeeded = test_result.as_ref().is_ok_and(|output| output.is_success());

    // Close the pool and drop the test database, unless a failed test asked to keep it.
    let drop_db = test_succeeded || !args.keep_db_on_failure;
    let cleanup_result = match cleanup_test_db(&service_connect_opts, &pool, drop_db).await {
        // The test's own failure (panic or `Err`) is what the test framework must report; a
        // cleanup error must not replace it, so it is only printed (the test framework shows
        // the captured output of failed tests).
        Err(err) if !test_succeeded => {
            eprintln!("sqlx-pg-test-template: failed to clean up test database `{db_name}`: {err}");
            Ok(())
        }
        result => result,
    };

    let output = match test_result {
        // Don't return the panic as a Result, but re-raise it to fail the test.
        // This ensures that the test failure is properly reported by the test framework.
        Err(payload) => std::panic::resume_unwind(payload),
        Ok(output) => output,
    };

    // A successful test must not leak its database, so a cleanup error fails the test.
    cleanup_result?;

    Ok(output)
}

/// Synchronous wrapper that executes a test lifecycle within a runtime block.
///
/// The test function may accept either a `Pool<Postgres>` or any type `P` that
/// implements `From<Pool<Postgres>>`, and may return `()` or a `Result` (see
/// [`TestTermination`]). The test's own return value is passed through, so the generated
/// `#[test]` function can hand it to the test framework.
///
/// # Arguments
///
/// * `f` - The asynchronous test function to execute.
/// * `args` - Test configuration parameters.
///
/// # Panics
///
/// Panics if the runner fails to set up the test database or to clean up after a successful
/// test, and re-raises a panic of the test itself.
pub fn run_test<F, Fut, P>(f: F, args: TestArgs) -> Fut::Output
where
    P: From<Pool<Postgres>>,
    F: Fn(P) -> Fut,
    Fut: std::future::Future,
    Fut::Output: TestTermination,
{
    sqlx::test_block_on(async move {
        match wrap_run_test(f, args).await {
            Err(e) => panic!("test failed: {e}"),
            Ok(output) => output,
        }
    })
}
