// SPDX-License-Identifier: Apache-2.0 OR MIT

//! syn-serde3 0.0.1 (https://github.com/taiki-e/syn-serde), copied in so surface-spec can fix
//! it: the `Ident` adapter converts raw identifiers such as `r#type` back with
//! `proc_macro2::Ident::new_raw`, where upstream panics in `proc_macro2::Ident::new`. Licensed
//! Apache-2.0 OR MIT; see `LICENSE-APACHE` and `LICENSE-MIT` beside this file.

#![allow(
    clippy::derivable_impls,
    clippy::enum_glob_use,
    clippy::used_underscore_binding,
    clippy::wildcard_imports
)]

#[macro_use]
mod macros;

#[path = "gen/ast_struct.rs"]
mod ast_struct;

#[path = "gen/ast_enum.rs"]
mod ast_enum;

#[path = "gen/convert.rs"]
mod convert;

mod attr {
    pub use crate::tools::surface_spec::syn_serde::{
        ast_enum::{AttrStyle, Meta},
        ast_struct::{Attribute, MetaList, MetaNameValue},
    };
}
#[doc(hidden)]
pub use self::attr::{AttrStyle, Attribute, Meta, MetaList, MetaNameValue};

mod data;
pub(crate) use self::data::assert_struct_semi;
#[doc(hidden)]
pub use self::data::{Field, Fields, FieldsNamed, FieldsUnnamed, Variant};

mod expr;
#[doc(hidden)]
pub use self::expr::{
    Arm, Expr, ExprArray, ExprAssign, ExprAsync, ExprAwait, ExprBinary, ExprBlock, ExprBreak,
    ExprCall, ExprCast, ExprClosure, ExprConst, ExprContinue, ExprField, ExprForLoop, ExprGroup,
    ExprIf, ExprIndex, ExprInfer, ExprLet, ExprLit, ExprLoop, ExprMacro, ExprMatch, ExprMethodCall,
    ExprParen, ExprPath, ExprRange, ExprReference, ExprRepeat, ExprReturn, ExprStruct, ExprTry,
    ExprTryBlock, ExprTuple, ExprUnary, ExprUnsafe, ExprWhile, ExprYield, FieldValue, Index, Label,
    Member, RangeLimits,
};

mod file {
    pub use crate::tools::surface_spec::syn_serde::ast_struct::File;
}
#[doc(hidden)]
pub use self::file::File;

mod generics;
pub use self::ast_struct::PredicateType;
#[doc(hidden)]
pub use self::generics::{
    BoundLifetimes, ConstParam, GenericParam, Generics, LifetimeParam, PredicateLifetime,
    TraitBound, TraitBoundModifier, TypeParam, TypeParamBound, WhereClause, WherePredicate,
};

mod item;
#[doc(hidden)]
pub use self::item::{
    FnArg, ForeignItem, ForeignItemFn, ForeignItemMacro, ForeignItemStatic, ForeignItemType,
    ImplItem, ImplItemConst, ImplItemFn, ImplItemMacro, ImplItemType, ImplRestriction, Item,
    ItemConst, ItemEnum, ItemExternCrate, ItemFn, ItemForeignMod, ItemImpl, ItemMacro, ItemMod,
    ItemStatic, ItemStruct, ItemTrait, ItemTraitAlias, ItemType, ItemUnion, ItemUse, Receiver,
    Signature, StaticMutability, TraitItem, TraitItemConst, TraitItemFn, TraitItemMacro,
    TraitItemType, UseGroup, UseName, UsePath, UseRename, UseTree, Variadic,
};

mod lifetime {
    pub use crate::tools::surface_spec::syn_serde::ast_struct::Lifetime;
}
#[doc(hidden)]
pub use self::lifetime::Lifetime;

mod lit;
#[doc(hidden)]
pub use self::lit::{Lit, LitBool, LitByte, LitByteStr, LitChar, LitFloat, LitInt, LitStr};

mod mac {
    pub use crate::tools::surface_spec::syn_serde::{ast_enum::MacroDelimiter, ast_struct::Macro};
}
#[doc(hidden)]
pub use self::mac::{Macro, MacroDelimiter};

mod op {
    pub use crate::tools::surface_spec::syn_serde::ast_enum::{BinOp, UnOp};
}
#[doc(hidden)]
pub use self::op::{BinOp, UnOp};

mod pat;
#[doc(hidden)]
pub use self::pat::{
    FieldPat, Pat, PatIdent, PatOr, PatParen, PatReference, PatRest, PatSlice, PatStruct, PatTuple,
    PatTupleStruct, PatType, PatWild,
};

mod path;
#[doc(hidden)]
pub use self::path::{
    AngleBracketedGenericArguments, AssocConst, AssocType, Constraint, GenericArgument,
    ParenthesizedGenericArguments, Path, PathArguments, PathSegment, QSelf,
};

mod restriction;
#[doc(hidden)]
pub use self::restriction::{FieldMutability, VisRestricted, Visibility};

mod stmt;
#[doc(hidden)]
pub use self::stmt::{Block, Local, LocalInit, Stmt, StmtMacro};

mod ty;
#[doc(hidden)]
pub use self::ty::{
    Abi, FnPtrVariadic, NamedArg, ReturnType, Type, TypeArray, TypeFnPtr, TypeGroup, TypeImplTrait,
    TypeMacro, TypeParen, TypePath, TypePtr, TypeReference, TypeSlice, TypeTraitObject, TypeTuple,
};

mod token_stream;
#[doc(hidden)]
pub use self::token_stream::{Ident, Literal, TokenStream};

#[path = "gen/convert_manual.rs"]
mod convert_manual;

mod sealed {
    #[allow(unknown_lints, unnameable_types)] // Not public API. unnameable_types is available on Rust 1.79+
    pub trait Sealed {}
}

// -----------------------------------------------------------------------------
// Syn trait

/// A trait for the data structures of [Syn] and [proc-macro2].
///
/// [Syn]: https://github.com/dtolnay/syn
/// [proc-macro2]: https://github.com/alexcrichton/proc-macro2
pub trait Syn: Sized + sealed::Sealed {
    type Adapter: Serialize + for<'de> Deserialize<'de>;

    /// Converts a `Syn` type into an adapter.
    fn to_adapter(&self) -> Self::Adapter;

    /// Converts an adapter into a `Syn` type.
    fn from_adapter(adapter: &Self::Adapter) -> Self;
}

// -----------------------------------------------------------------------------

use core::ops;
use std::{boxed::Box, string::String, vec::Vec};

use proc_macro2::Span;
use serde::{Deserialize, Serialize};

type Punctuated<T> = Vec<T>;

fn default<T>() -> T
where
    T: Default,
{
    T::default()
}

fn default_or_none<T>(x: bool) -> Option<T>
where
    T: Default,
{
    if x { Some(T::default()) } else { None }
}

fn not<T>(x: T) -> T::Output
where
    T: ops::Not,
{
    !x
}

// https://github.com/rust-lang/rust/issues/51443
trait RefInto<U>: Sized {
    fn ref_into<'a>(&'a self) -> U
    where
        &'a Self: Into<U>,
    {
        self.into()
    }
}

impl<T, U> RefInto<U> for T {}

trait MapInto<U, M> {
    type T;

    fn ref_map<'a, F>(&'a self, f: F) -> M
    where
        Self::T: 'a,
        F: FnMut(&'a Self::T) -> U;

    fn map_into<'a>(&'a self) -> M
    where
        Self::T: 'a,
        &'a Self::T: Into<U>,
    {
        self.ref_map(Into::into)
    }
}

impl<T, U> MapInto<U, Vec<U>> for Vec<T> {
    type T = T;

    fn ref_map<'a, F>(&'a self, f: F) -> Vec<U>
    where
        F: FnMut(&'a Self::T) -> U,
    {
        self.iter().map(f).collect()
    }
}

impl<T, U, P> MapInto<U, syn::punctuated::Punctuated<U, P>> for Vec<T>
where
    P: Default,
{
    type T = T;

    fn ref_map<'a, F>(&'a self, f: F) -> syn::punctuated::Punctuated<U, P>
    where
        F: FnMut(&'a Self::T) -> U,
    {
        self.iter().map(f).collect()
    }
}

impl<T, U, P> MapInto<U, Vec<U>> for syn::punctuated::Punctuated<T, P>
where
    P: Default,
{
    type T = T;

    fn ref_map<'a, F>(&'a self, f: F) -> Vec<U>
    where
        F: FnMut(&'a Self::T) -> U,
    {
        self.iter().map(f).collect()
    }
}

impl<T, U> MapInto<U, Option<U>> for Option<T> {
    type T = T;

    fn ref_map<'a, F>(&'a self, f: F) -> Option<U>
    where
        F: FnMut(&'a Self::T) -> U,
    {
        self.as_ref().map(f)
    }
}

impl<T, U> MapInto<U, Box<U>> for Box<T> {
    type T = T;

    fn ref_map<'a, F>(&'a self, mut f: F) -> Box<U>
    where
        F: FnMut(&'a Self::T) -> U,
    {
        Box::new(f(self))
    }
}
