# sqlx-pg-test-template

## Goal

Provide a faster alternative to the standard `#[sqlx::test]` macro for PostgreSQL integration testing.

## Description

`sqlx-pg-test-template` optimizes test suite performance by using PostgreSQL database templates. Instead of running full migrations for every individual test, this crate creates a new database from a specified template using the `CREATE DATABASE ... WITH TEMPLATE ...` command. This approach significantly reduces the overhead of database setup in large test suites.

### Key Features

- **Fast Setup**: Creating a database from a template is much faster than running migrations.
- **Isolate Tests**: Each test runs in its own dedicated database.
- **Automatic Clean-up**: Databases are automatically dropped after the test completes.
- **Parallel Execution Support**: Database names are unique to prevent collisions during parallel test runs.

## Usage

### 1. Configure the Macro

Annotate your asynchronous test functions with `#[sqlx_pg_test_template::test]`.

```rust
use sqlx_pg_test_template::test;
use sqlx::{Pool, Postgres};

// Basic usage: uses the database from DATABASE_URL as the template
#[sqlx_pg_test_template::test]
async fn test_basic(pool: Pool<Postgres>) {
    // Your test logic here
}

// Specify a custom template database
#[sqlx_pg_test_template::test(template = "my_seed_template")]
async fn test_with_custom_template(pool: Pool<Postgres>) {
    // Your test logic here
}

// Set maximum pool connections for a specific test
#[sqlx_pg_test_template::test(max_connections = 5)]
async fn test_with_custom_pool(pool: Pool<Postgres>) {
    // Your test logic here
}
```

### 2. Run Tests

Set the `DATABASE_URL` environment variable to point to your template database and run `cargo test`.

```sh
DATABASE_URL="postgres://user:pass@localhost:5432/template_db" cargo test
```

## Requirements

- **Permissions**: The PostgreSQL user must have `CREATEDB` permissions.
- **Default Database**: A default database (e.g., `postgres`) must be accessible to the user to manage test databases.
- **Template Database**: You must manually create and maintain the template database (e.g., run migrations on it before running tests).

## Credits & Acknowledgements

This project is a fork of the original repository 
[sqlx_pg_test_template](https://github.com/gzigzigzeo/sqlx-pg-test-template) created by 
[Viktor Sokolov](https://github.com/gzigzigzeo).

We thank the original developer for his excellent work and contribution to the open-source community.

## Licence

This project is licensed under the MIT Licence.

Original work Copyright © 2014 Viktor Sokolov
Modified work Copyright © 2026 Onside.io

See the [LICENCE](LICENCE) file for the full licence text and copyright notices.
