//! # sqlx-pg-test-template-macros
//!
//! This crate provides the procedural macro `#[test]` which simplifies writing
//! PostgreSQL integration tests by automatically creating and cleaning up
//! template-based databases.

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse::Parser, MetaNameValue};

type AttributeArgs = syn::punctuated::Punctuated<syn::Meta, syn::Token![,]>;
type Error = Box<dyn std::error::Error>;
type Result<T> = std::result::Result<T, Error>;

/// Internal representation of the macro arguments.
#[derive(Default)]
struct Args {
    /// Name of the template database to use.
    template_name: Option<String>,
    /// Maximum number of connections for the test pool.
    max_connections: Option<u32>,
}

/// Procedural macro that enables template-based database tests.
///
/// This macro wraps a test function. It sets up a new PostgreSQL database from a template,
/// provides a connection pool to the test, and ensures the database is dropped afterward.
///
/// # Examples
///
/// ```rust
/// use sqlx::{Pool, Postgres};
///
/// #[sqlx_pg_test_template::test]
/// async fn my_test(pool: Pool<Postgres>) {
///     // ...
/// }
///
/// #[sqlx_pg_test_template::test(template = "custom_template", max_connections = 10)]
/// async fn complex_test(pool: Pool<Postgres>) {
///     // ...
/// }
/// ```
#[proc_macro_attribute]
pub fn test(args: TokenStream, input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::ItemFn);
    let args = args;

    match expand(args, input) {
        Ok(ts) => ts,
        Err(e) => {
            if let Some(parse_err) = e.downcast_ref::<syn::Error>() {
                parse_err.to_compile_error().into()
            } else {
                let msg = e.to_string();
                quote!(::std::compile_error!(#msg)).into()
            }
        }
    }
}

/// Parses the macro arguments and expands the test function.
fn expand(args: TokenStream, input: syn::ItemFn) -> Result<TokenStream> {
    let parser = AttributeArgs::parse_terminated;
    let args = parser.parse2(args.into())?;
    let args = parse_args(args)?;

    expand_with_args(input, args)
}

/// Parses the raw attribute arguments into a structured `Args` struct.
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

            arg => {
                return Err(syn::Error::new_spanned(
                    arg,
                    r#"expected `template = "database_name"` and/or `max_connections = 5`"#,
                ))
            }
        }
    }

    Ok(args)
}

/// Generates the final code for the test function, including the runner orchestration.
fn expand_with_args(input: syn::ItemFn, args: Args) -> Result<TokenStream> {
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

    let name_str = name.to_string();

    Ok(quote! {
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
    .into())
}

/// Helper to parse an expression into a string literal.
fn parse_lit_str(expr: &syn::Expr) -> syn::Result<String> {
    match expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(lit),
            ..
        }) => Ok(lit.value()),
        _ => Err(syn::Error::new_spanned(expr, "expected string")),
    }
}

/// Helper to parse an expression into an integer literal (as a string).
fn parse_lit_int(expr: &syn::Expr) -> syn::Result<String> {
    match expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(lit),
            ..
        }) => Ok(lit.base10_digits().to_owned()),
        _ => Err(syn::Error::new_spanned(expr, "expected integer")),
    }
}
