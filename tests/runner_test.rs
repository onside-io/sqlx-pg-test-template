//! Runtime behaviour of the runner that the macro-based tests cannot express.

use std::str::FromStr;

use sqlx::postgres::PgConnectOptions;
use sqlx::{Connection, PgConnection, Pool, Postgres};
use sqlx_pg_test_template_runner::{Error, TestArgs, wrap_run_test};

/// Cloning a template that has other sessions is reported with a hint instead of the raw
/// PostgreSQL error.
///
/// The test holds its own session on the `postgres` database and then asks the runner to clone
/// that database. The session is the "other user" PostgreSQL complains about, so the outcome
/// does not depend on anything else running on the server. The other tests clone `template1`,
/// so the extra session does not disturb them.
#[tokio::test]
async fn template_in_use_is_reported_with_a_hint() {
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let blocker_opts = PgConnectOptions::from_str(&database_url)
        .expect("DATABASE_URL must be a valid PostgreSQL URL")
        .database("postgres");
    let blocker = PgConnection::connect_with(&blocker_opts)
        .await
        .expect("connect to the postgres database");

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
