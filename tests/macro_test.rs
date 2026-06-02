//! Those tests ensure that macro compiles

// For now, now CI
#[cfg(test)]
mod test {
    #[sqlx_pg_test_template::test]
    async fn test_macro_default_custom(_pool: sqlx::Pool<sqlx::Postgres>) {}

    #[sqlx_pg_test_template::test]
    #[should_panic(expected = "test")]
    async fn test_macro_default_custom_panic(_pool: sqlx::Pool<sqlx::Postgres>) {
        panic!("test");
    }

    #[sqlx_pg_test_template::test(max_connections = 5)]
    async fn test_macro_default_custom_mc(_pool: sqlx::Pool<sqlx::Postgres>) {}

    #[sqlx_pg_test_template::test(keep_db_on_failure = true)]
    async fn test_macro_default_custom_keep_db(_pool: sqlx::Pool<sqlx::Postgres>) {}

    #[sqlx_pg_test_template::test(template = "postgres")]
    async fn test_macro_default_custom_mc_tpl(_pool: sqlx::Pool<sqlx::Postgres>) {}
}
