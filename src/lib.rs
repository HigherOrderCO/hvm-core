#![feature(const_type_id, extern_types, inline_const, generic_const_exprs, new_uninit)]
#![cfg_attr(feature = "trace", feature(const_type_name))]
#![allow(non_snake_case, incomplete_features)]
#![feature(stmt_expr_attributes)] // temp

pub mod ast;
pub mod compile;
pub mod host;
pub mod ops;
pub mod run;
pub mod stdlib;
pub mod transform;
pub mod util;

#[doc(hidden)] // not public api
pub mod fuzz;
#[doc(hidden)] // not public api
pub mod trace;

#[doc(hidden)] // shim for compilation
pub mod gen;
