// File: macros/src/lib.rs
// Purpose: Implements procedural macros for PostgreSQL template-based testing.

//! # sqlx-pg-test-template-macros
//!
//! Provides the `#[test]` procedural macro attribute.
//! It automates database creation from a template and handles clean-up after tests.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{MetaNameValue, parse::Parser};

/// Arguments for syn punctuated parsing.
type AttributeArgs = syn::punctuated::Punctuated<syn::Meta, syn::Token![,]>;

/// Configuration options for the `#[test]` macro.
#[derive(Default)]
struct Args {
    /// Name of the template database to clone.
    template_name: Option<String>,
    /// Maximum concurrent connections in the test pool.
    max_connections: Option<u32>,
    /// If true, the test database is not dropped if the test fails.
    keep_db_on_failure: Option<bool>,
}

/// Wraps a test function to enable template-based PostgreSQL testing.
///
/// This macro creates a temporary database from a template before the test runs.
/// It provides a `sqlx::Pool<Postgres>` to the test function and drops the database
/// after the test completes (unless `keep_db_on_failure` is set for failed tests).
///
/// The test function argument may also be any type implementing
/// `From<sqlx::Pool<Postgres>>`, letting you inject an application-specific
/// wrapper around the pool. The runner keeps its own pool for cleanup, so
/// teardown is unaffected.
///
/// # Parameters
///
/// - `args`: Optional configuration (e.g., `template`, `max_connections`).
/// - `input`: The test function to be wrapped.
///
/// # Examples
///
/// Using the raw pool:
///
/// ```ignore
/// use sqlx::{Pool, Postgres};
///
/// #[sqlx_pg_test_template::test]
/// async fn my_test(pool: Pool<Postgres>) {
///     // ...
/// }
/// ```
///
/// Using a wrapper type:
///
/// ```ignore
/// use sqlx::{Pool, Postgres};
///
/// #[derive(Clone)]
/// struct AppDb {
///     pool: Pool<Postgres>,
/// }
///
/// impl From<Pool<Postgres>> for AppDb {
///     fn from(pool: Pool<Postgres>) -> Self {
///         Self { pool }
///     }
/// }
///
/// #[sqlx_pg_test_template::test]
/// async fn my_wrapped_test(db: AppDb) {
///     // ...
/// }
/// ```
#[proc_macro_attribute]
pub fn test(
    args: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    expand(args.into(), input.into()).into()
}

/// Expands the attribute on `proc_macro2` token streams.
///
/// This is the entry point shared by [`test`] and the unit tests: unlike `proc_macro`,
/// `proc_macro2` tokens can be built outside of a macro invocation. Every error is rendered
/// as a `compile_error!` invocation, so the output is always a valid item.
///
/// # Arguments
///
/// * `args` - The raw attribute arguments.
/// * `input` - The annotated item.
///
/// # Returns
///
/// Returns the generated test function, or a `compile_error!` describing the rejected input.
fn expand(args: TokenStream, input: TokenStream) -> TokenStream {
    try_expand(args, input).unwrap_or_else(syn::Error::into_compile_error)
}

/// Parses the annotated function and the attribute arguments, then generates the test wrapper.
///
/// The function item is parsed before the arguments, so an item that is not a function is
/// reported even when the arguments are malformed as well.
///
/// # Arguments
///
/// * `args` - The raw attribute arguments.
/// * `input` - The annotated item, expected to be a function.
///
/// # Returns
///
/// Returns the expanded `TokenStream` or a parse error.
fn try_expand(args: TokenStream, input: TokenStream) -> syn::Result<TokenStream> {
    let input: syn::ItemFn = syn::parse2(input)?;
    let args = AttributeArgs::parse_terminated.parse2(args)?;
    let args = parse_args(args)?;

    Ok(expand_with_args(input, args))
}

/// Converts parsed metadata into the `Args` configuration struct.
///
/// # Arguments
///
/// * `attr_args` - Punctuated list of metadata attributes.
///
/// # Returns
///
/// Returns the populated `Args` struct or a `syn::Result` error.
fn parse_args(attr_args: AttributeArgs) -> syn::Result<Args> {
    let mut args = Args::default();

    for arg in attr_args {
        let path = arg.path().clone();

        match arg {
            syn::Meta::NameValue(MetaNameValue { value, .. }) if path.is_ident("template") => {
                args.template_name = Some(parse_lit_str(&value)?);
            }

            syn::Meta::NameValue(MetaNameValue { value, .. })
                if path.is_ident("max_connections") =>
            {
                let digits = parse_lit_int(&value)?;
                let mc: u32 = digits
                    .parse()
                    .map_err(|_| syn::Error::new_spanned(value, "expected u32 number"))?;

                args.max_connections = Some(mc);
            }

            syn::Meta::NameValue(MetaNameValue { value, .. })
                if path.is_ident("keep_db_on_failure") =>
            {
                let flag = parse_lit_bool(&value)?;

                args.keep_db_on_failure = Some(flag);
            }

            arg => {
                return Err(syn::Error::new_spanned(
                    arg,
                    r#"expected `template = "database_name"` and/or `max_connections = 5` and/or `keep_db_on_failure = true`"#,
                ));
            }
        }
    }

    Ok(args)
}

/// Generates the code that executes the test within the runner.
///
/// # Arguments
///
/// * `input` - The original test function.
/// * `args` - Parsed configuration arguments.
///
/// # Returns
///
/// Returns a `TokenStream` containing the generated code.
fn expand_with_args(input: syn::ItemFn, args: Args) -> TokenStream {
    let ret = &input.sig.output;
    let name = &input.sig.ident;
    let inputs = &input.sig.inputs;
    let body = &input.block;
    let attrs = &input.attrs;

    let template_name = match args.template_name {
        None => quote! { None },
        Some(name) => quote! { Some(#name.to_string()) },
    };

    let max_connections = match args.max_connections {
        None => quote! { None },
        Some(mc) => quote! { Some(#mc) },
    };

    let keep_db_on_failure = match args.keep_db_on_failure {
        None => quote! { false },
        Some(flag) => quote! { #flag },
    };

    let name_str = name.to_string();

    quote! {
        #(#attrs)*
        #[::core::prelude::v1::test]
        fn #name() #ret {
            async fn #name(#inputs) #ret {
                #body
            };

            let test_args = ::sqlx_pg_test_template::TestArgs {
                template_name: #template_name,
                max_connections: #max_connections,
                module_path: format!("{}::{}", module_path!().to_string(), #name_str),
                keep_db_on_failure: #keep_db_on_failure,
            };

            sqlx_pg_test_template::run_test(#name, test_args)

            // TODO: check timeout of pool going out of scope. main problem is that sqlx does
            // not export core trait.
            //
            // let close_timed_out = sqlx::rt::timeout(Duration::from_secs(10), pool.close())
            //     .await
            //     .is_err();

            // if close_timed_out {
            //     eprintln!("test {test_path} held onto Pool after exiting");
            // }

        }
    }
}

/// Extracts a string value from a literal expression.
///
/// # Arguments
///
/// * `expr` - The expression to parse.
///
/// # Returns
///
/// Returns the string value or an error if the expression is not a string literal.
fn parse_lit_str(expr: &syn::Expr) -> syn::Result<String> {
    match expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(lit),
            ..
        }) => Ok(lit.value()),
        _ => Err(syn::Error::new_spanned(expr, "expected string")),
    }
}

/// Extracts an integer value (as a string) from a literal expression.
///
/// # Arguments
///
/// * `expr` - The expression to parse.
///
/// # Returns
///
/// Returns the base-10 string representation of the integer or an error.
fn parse_lit_int(expr: &syn::Expr) -> syn::Result<String> {
    match expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(lit),
            ..
        }) => Ok(lit.base10_digits().to_owned()),
        _ => Err(syn::Error::new_spanned(expr, "expected integer")),
    }
}

/// Extracts a boolean value from a literal expression.
///
/// # Arguments
///
/// * `expr` - The expression to parse.
///
/// # Returns
///
/// Returns the boolean value or an error if the expression is not a boolean literal.
fn parse_lit_bool(expr: &syn::Expr) -> syn::Result<bool> {
    match expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Bool(lit),
            ..
        }) => Ok(lit.value()),
        _ => Err(syn::Error::new_spanned(expr, "expected bool")),
    }
}

#[cfg(test)]
mod tests;
