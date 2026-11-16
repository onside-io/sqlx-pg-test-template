//! Runtime behaviour of the runner that the macro-based tests cannot express.

use std::str::FromStr;

use sqlx::postgres::PgConnectOptions;
use sqlx::{Connection, PgConnection, Pool, Postgres};
use sqlx_pg_test_template_runner::{Error, TestArgs, wrap_run_test};

/// Connects to the `postgres` database, from which the runner's test databases can be inspected.
///
/// Sessions on `postgres` do not disturb the other tests, because they clone `template1`.
async fn maintenance_connection() -> PgConnection {
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let opts = PgConnectOptions::from_str(&database_url)
        .expect("DATABASE_URL must be a valid PostgreSQL URL")
        .database("postgres");

    PgConnection::connect_with(&opts)
        .await
        .expect("connect to the postgres database")
}

/// Finds the database the runner created for `module_path`.
///
/// The runner stores the module path as the database comment, so the lookup does not depend
/// on the hashing scheme behind the database name.
async fn find_test_db(conn: &mut PgConnection, module_path: &str) -> Option<String> {
    sqlx::query_scalar(
        "SELECT d.datname::text FROM pg_database d \
         JOIN pg_shdescription s ON s.objoid = d.oid AND s.classoid = 'pg_database'::regclass \
         WHERE s.description = $1",
    )
    .bind(module_path)
    .fetch_optional(conn)
    .await
    .expect("look up the test database by its comment")
}

/// Drops the database the runner created for `module_path`, if it still exists.
///
/// Returns the name of the dropped database. Dropping before asserting on the result keeps a
/// failed assertion from leaking the database.
async fn drop_test_db(module_path: &str) -> Option<String> {
    let mut conn = maintenance_connection().await;
    let db_name = find_test_db(&mut conn, module_path).await;
    if let Some(db_name) = &db_name {
        sqlx::query(&format!("DROP DATABASE IF EXISTS {db_name} WITH (FORCE)"))
            .execute(&mut conn)
            .await
            .expect("drop the test database");
    }
    conn.close()
        .await
        .expect("close the maintenance connection");

    db_name
}

/// Arguments that clone the database named in `DATABASE_URL`.
fn test_args(module_path: &str, keep_db_on_failure: bool) -> TestArgs {
    TestArgs {
        template_name: None,
        max_connections: None,
        module_path: module_path.to_string(),
        keep_db_on_failure,
    }
}

/// Cloning a template that has other sessions is reported with a hint instead of the raw
/// PostgreSQL error.
///
/// The test holds its own session on the `postgres` database and then asks the runner to clone
/// that database. The session is the "other user" PostgreSQL complains about, so the outcome
/// does not depend on anything else running on the server. The other tests clone `template1`,
/// so the extra session does not disturb them.
#[tokio::test]
async fn template_in_use_is_reported_with_a_hint() {
    let blocker = maintenance_connection().await;

    let args = TestArgs {
        template_name: Some("postgres".to_string()),
        max_connections: None,
        module_path: "runner_test::template_in_use_is_reported_with_a_hint".to_string(),
        keep_db_on_failure: false,
    };

    let err = wrap_run_test(|_pool: Pool<Postgres>| async {}, args)
        .await
        .expect_err("cloning a template with an open session must fail");

    assert!(
        matches!(err, Error::TemplateInUse { ref template, .. } if template == "postgres"),
        "unexpected error: {err:?}"
    );
    assert!(
        err.to_string().contains("DATABASE_URL"),
        "the error should tell how to fix the setup: {err}"
    );

    blocker.close().await.expect("close the blocking session");
}

/// An `Err` returned by the test is the test's own outcome, not a runner error, and the
/// database is dropped like after any other test.
#[tokio::test]
async fn returned_err_is_passed_through_and_db_is_dropped() {
    let module_path = "runner_test::returned_err_is_passed_through_and_db_is_dropped";

    let outcome = wrap_run_test(
        |_pool: Pool<Postgres>| async { Err::<(), sqlx::Error>(sqlx::Error::RowNotFound) },
        test_args(module_path, false),
    )
    .await
    .expect("the runner itself must succeed when the test returns Err");

    assert!(
        matches!(outcome, Err(sqlx::Error::RowNotFound)),
        "unexpected outcome: {outcome:?}"
    );
    assert_eq!(
        drop_test_db(module_path).await,
        None,
        "the database must be dropped after a test that returned Err"
    );
}

/// With `keep_db_on_failure`, a returned `Err` counts as a failure and the database is kept.
#[tokio::test]
async fn returned_err_keeps_db_when_requested() {
    let module_path = "runner_test::returned_err_keeps_db_when_requested";

    let outcome = wrap_run_test(
        |_pool: Pool<Postgres>| async { Err::<(), sqlx::Error>(sqlx::Error::RowNotFound) },
        test_args(module_path, true),
    )
    .await
    .expect("the runner itself must succeed when the test returns Err");

    assert!(outcome.is_err(), "unexpected outcome: {outcome:?}");
    assert!(
        drop_test_db(module_path).await.is_some(),
        "the database must be kept after a test that returned Err"
    );
}

/// `keep_db_on_failure` does not keep the database of a test that returned `Ok`.
#[tokio::test]
async fn returned_ok_drops_db_even_with_keep_db_on_failure() {
    let module_path = "runner_test::returned_ok_drops_db_even_with_keep_db_on_failure";

    let outcome = wrap_run_test(
        |_pool: Pool<Postgres>| async { Ok::<(), sqlx::Error>(()) },
        test_args(module_path, true),
    )
    .await
    .expect("the runner itself must succeed when the test returns Ok");

    assert!(outcome.is_ok(), "unexpected outcome: {outcome:?}");
    assert_eq!(
        drop_test_db(module_path).await,
        None,
        "the database must be dropped after a test that returned Ok"
    );
}
