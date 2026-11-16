//! Those tests ensure that macro compiles
//!
//! `DATABASE_URL` must name a database that nothing connects to (see "Development" in README.md).

// For now, no CI
#[cfg(test)]
mod test {
    #[sqlx_pg_test_template::test]
    async fn test_macro_default_custom(_pool: sqlx::Pool<sqlx::Postgres>) {}

    #[sqlx_pg_test_template::test]
    #[should_panic(expected = "intentional panic")]
    async fn test_macro_default_custom_panic(_pool: sqlx::Pool<sqlx::Postgres>) {
        panic!("intentional panic");
    }

    #[sqlx_pg_test_template::test(max_connections = 5)]
    async fn test_macro_default_custom_mc(_pool: sqlx::Pool<sqlx::Postgres>) {}

    #[sqlx_pg_test_template::test(keep_db_on_failure = true)]
    async fn test_macro_default_custom_keep_db(_pool: sqlx::Pool<sqlx::Postgres>) {}

    // `template1` exists on every PostgreSQL server and has no sessions, so it can always be cloned.
    #[sqlx_pg_test_template::test(template = "template1")]
    async fn test_macro_default_custom_mc_tpl(_pool: sqlx::Pool<sqlx::Postgres>) {}

    // The return type is copied to the generated test, so `?` works with any `E: Debug`.
    #[sqlx_pg_test_template::test]
    async fn test_macro_result_sqlx_error(
        pool: sqlx::Pool<sqlx::Postgres>,
    ) -> Result<(), sqlx::Error> {
        let row: (i32,) = sqlx::query_as("SELECT 1").fetch_one(&pool).await?;
        assert_eq!(row.0, 1);
        Ok(())
    }

    #[sqlx_pg_test_template::test]
    async fn test_macro_result_boxed_error(
        pool: sqlx::Pool<sqlx::Postgres>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let row: (i32,) = sqlx::query_as("SELECT 1").fetch_one(&pool).await?;
        assert_eq!(row.0, 1);
        Ok(())
    }

    // Attribute arguments and a `Result` return type (via the `sqlx::Result` alias) combine.
    #[sqlx_pg_test_template::test(keep_db_on_failure = true)]
    async fn test_macro_result_keep_db(_pool: sqlx::Pool<sqlx::Postgres>) -> sqlx::Result<()> {
        Ok(())
    }
}
