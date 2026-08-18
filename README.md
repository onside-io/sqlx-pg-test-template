# sqlx-pg-test-template

## Goal

Provide a fast alternative to the standard `#[sqlx::test]` macro for PostgreSQL integration testing.

## Description

`sqlx-pg-test-template` optimizes test suite performance by using PostgreSQL database templates. Instead of running 
migrations for every test, it creates a new database from a template. This approach reduces database setup time 
in large test suites.

### Key Features

- **Fast Setup**: Creating a database from a template is faster than running migrations.
- **Test Isolation**: Each test runs in a dedicated database.
- **Automatic Cleanup**: Databases are dropped after the test completes by default.
- **Parallel Execution**: Uses unique database names to prevent collisions.

## Usage

### 1. Configure the Macro

Annotate test functions with `#[sqlx_pg_test_template::test]`.

```rust
use sqlx_pg_test_template::test;
use sqlx::{Pool, Postgres};

// Basic usage: uses the database from DATABASE_URL as the template
#[sqlx_pg_test_template::test]
async fn test_basic(pool: Pool<Postgres>) {
    // ...
}

// Specify a custom template database
#[sqlx_pg_test_template::test(template = "my_seed_template")]
async fn test_with_custom_template(pool: Pool<Postgres>) {
    // ...
}

// Set maximum pool connections
#[sqlx_pg_test_template::test(max_connections = 5)]
async fn test_with_custom_pool(pool: Pool<Postgres>) {
    // ...
}

// Keep db for debug
#[sqlx_pg_test_template::test(keep_db_on_failure = true)]
async fn test_with_debug(pool: Pool<Postgres>) {
    // ...
    panic!("unexpected assert")
}
```

### 2. Use a Custom Pool Wrapper (optional)

The injected argument is not limited to `Pool<Postgres>`. Any type implementing
`From<Pool<Postgres>>` works, which is convenient when your application passes a
newtype or a struct holding the pool.

```rust
use sqlx::{Pool, Postgres};

#[derive(Clone)]
struct AppDb {
    pool: Pool<Postgres>,
}

impl From<Pool<Postgres>> for AppDb {
    fn from(pool: Pool<Postgres>) -> Self {
        Self { pool }
    }
}

// Raw pool
#[sqlx_pg_test_template::test]
async fn test_with_raw_pool(pool: Pool<Postgres>) {
    // ...
}

// Wrapper type; conversion is automatic
#[sqlx_pg_test_template::test]
async fn test_with_wrapper(db: AppDb) {
    // ...
}
```

The runner keeps its own `Pool<Postgres>` internally, so cleanup behaves
identically for both forms.

### 3. Run Tests

Set `DATABASE_URL` to point to the template database and run `cargo test`.

```sh
DATABASE_URL="postgres://user:pass@localhost:5432/template_db" cargo test
```

## Requirements

- **Permissions**: The PostgreSQL user must have `CREATEDB` permissions.
- **Default Database**: A default database (e.g., `postgres`) must be accessible to manage test databases.
- **Template Database**: The template database must exist and be up-to-date.

## Credits & Acknowledgements

This project is a fork of the original repository 
[sqlx_pg_test_template](https://github.com/gzigzigzeo/sqlx-pg-test-template) created by 
[Viktor Sokolov](https://github.com/gzigzigzeo).

We thank the original developer for his excellent work and contribution to the open-source community.

## Licence

This project is licensed under the MIT License.

Original work Copyright © 2014 Viktor Sokolov
Modified work Copyright © 2026 Onside.io

See the [LICENSE](LICENSE) file for the full license text.
