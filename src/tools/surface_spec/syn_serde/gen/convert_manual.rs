// SPDX-License-Identifier: Apache-2.0 OR MIT

// Manual conversion impls for types not handled by codegen (syn 3 API changes).
// This file is NOT @generated and must not be overwritten by codegen.

#![cfg_attr(rustfmt, rustfmt::skip)]
use crate::tools::surface_spec::syn_serde::*;

// NamedArg (was BareFnArg in syn 2)
syn_trait_impl!(syn::NamedArg);
impl From<&syn::NamedArg> for NamedArg {
    fn from(node: &syn::NamedArg) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            name: node.name.ref_map(|(_0, _1)| (*_0).ref_into()),
            ty: node.ty.ref_into(),
        }
    }
}
impl From<&NamedArg> for syn::NamedArg {
    fn from(node: &NamedArg) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            name: node.name.ref_map(|_0| ((*_0).ref_into(), default())),
            ty: node.ty.ref_into(),
        }
    }
}

// FnPtrVariadic (was BareVariadic in syn 2)
impl crate::tools::surface_spec::syn_serde::sealed::Sealed for syn::FnPtrVariadic {}
impl crate::tools::surface_spec::syn_serde::Syn for syn::FnPtrVariadic {
    type Adapter = FnPtrVariadic;
    fn to_adapter(&self) -> Self::Adapter { FnPtrVariadic::from(self) }
    fn from_adapter(adapter: &Self::Adapter) -> Self { Self::from(adapter) }
}
impl From<&syn::FnPtrVariadic> for FnPtrVariadic {
    fn from(node: &syn::FnPtrVariadic) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            name: node.name.ref_map(|(_0, _1)| (*_0).ref_into()),
            comma: node.comma.is_some(),
        }
    }
}
impl From<&FnPtrVariadic> for syn::FnPtrVariadic {
    fn from(node: &FnPtrVariadic) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            name: node.name.ref_map(|_0| ((*_0).ref_into(), default())),
            dots: default(),
            comma: default_or_none(node.comma),
        }
    }
}

// TypeFnPtr (was TypeBareFn in syn 2)
impl crate::tools::surface_spec::syn_serde::sealed::Sealed for syn::TypeFnPtr {}
impl crate::tools::surface_spec::syn_serde::Syn for syn::TypeFnPtr {
    type Adapter = TypeFnPtr;
    fn to_adapter(&self) -> Self::Adapter { TypeFnPtr::from(self) }
    fn from_adapter(adapter: &Self::Adapter) -> Self { Self::from(adapter) }
}
impl From<&syn::TypeFnPtr> for TypeFnPtr {
    fn from(node: &syn::TypeFnPtr) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            lifetimes: node.lifetimes.map_into(),
            unsafety: node.unsafety.is_some(),
            abi: node.abi.map_into(),
            inputs: node.inputs.map_into(),
            variadic: node.variadic.map_into(),
            output: node.output.ref_into(),
        }
    }
}
impl From<&TypeFnPtr> for syn::TypeFnPtr {
    fn from(node: &TypeFnPtr) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            lifetimes: node.lifetimes.map_into(),
            unsafety: default_or_none(node.unsafety),
            abi: node.abi.map_into(),
            fn_token: default(),
            paren_token: default(),
            inputs: node.inputs.map_into(),
            variadic: node.variadic.map_into(),
            output: node.output.ref_into(),
        }
    }
}

// TypePtr: syn 3 uses PointerMutability enum instead of separate const_token + mutability
impl crate::tools::surface_spec::syn_serde::sealed::Sealed for syn::TypePtr {}
impl crate::tools::surface_spec::syn_serde::Syn for syn::TypePtr {
    type Adapter = TypePtr;
    fn to_adapter(&self) -> Self::Adapter { TypePtr::from(self) }
    fn from_adapter(adapter: &Self::Adapter) -> Self { Self::from(adapter) }
}
impl From<&syn::TypePtr> for TypePtr {
    fn from(node: &syn::TypePtr) -> Self {
        Self {
            const_token: matches!(node.mutability, syn::PointerMutability::Const(..)),
            mutability: matches!(node.mutability, syn::PointerMutability::Mut(..)),
            elem: node.elem.map_into(),
        }
    }
}
impl From<&TypePtr> for syn::TypePtr {
    fn from(node: &TypePtr) -> Self {
        Self {
            attrs: default(),
            star_token: default(),
            mutability: if node.mutability {
                syn::PointerMutability::Mut(default())
            } else {
                syn::PointerMutability::Const(default())
            },
            elem: node.elem.map_into(),
        }
    }
}

// ConstParam: syn 3 changed default from (eq_token, Expr) pair
syn_trait_impl!(syn::ConstParam);
impl From<&syn::ConstParam> for ConstParam {
    fn from(node: &syn::ConstParam) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            ident: node.ident.ref_into(),
            ty: node.ty.ref_into(),
            eq_token: node.default.is_some(),
            default: node.default.ref_map(|(_eq, expr)| expr.ref_into()),
        }
    }
}
impl From<&ConstParam> for syn::ConstParam {
    fn from(node: &ConstParam) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            const_token: default(),
            ident: node.ident.ref_into(),
            colon_token: default(),
            ty: node.ty.ref_into(),
            default: node.default.ref_map(|expr| (default(), expr.ref_into())),
        }
    }
}

// TypeParam: syn 3 changed default from (eq_token, Type) pair
syn_trait_impl!(syn::TypeParam);
impl From<&syn::TypeParam> for TypeParam {
    fn from(node: &syn::TypeParam) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            ident: node.ident.ref_into(),
            colon_token: node.colon_token.is_some(),
            bounds: node.bounds.map_into(),
            eq_token: node.default.is_some(),
            default: node.default.ref_map(|(_eq, ty)| ty.ref_into()),
        }
    }
}
impl From<&TypeParam> for syn::TypeParam {
    fn from(node: &TypeParam) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            ident: node.ident.ref_into(),
            colon_token: default_or_none(node.colon_token),
            bounds: node.bounds.map_into(),
            default: node.default.ref_map(|ty| (default(), ty.ref_into())),
        }
    }
}

// TraitBound: syn 3 restructured with modifiers + maybe
syn_trait_impl!(syn::TraitBound);
impl From<&syn::TraitBound> for TraitBound {
    fn from(node: &syn::TraitBound) -> Self {
        Self {
            paren_token: node.paren_token.is_some(),
            modifier: if node.maybe.is_some() {
                TraitBoundModifier::Maybe
            } else {
                TraitBoundModifier::None
            },
            lifetimes: node.lifetimes.map_into(),
            path: node.path.ref_into(),
        }
    }
}
impl From<&TraitBound> for syn::TraitBound {
    fn from(node: &TraitBound) -> Self {
        Self {
            paren_token: default_or_none(node.paren_token),
            lifetimes: node.lifetimes.map_into(),
            modifiers: default(),
            maybe: if node.modifier.is_none() { None } else { Some(default()) },
            path: node.path.ref_into(),
        }
    }
}

// ExprAsync: syn 3 added modifiers (BlockModifiers)
syn_trait_impl!(syn::ExprAsync);
impl From<&syn::ExprAsync> for ExprAsync {
    fn from(node: &syn::ExprAsync) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            capture: node.capture.is_some(),
            block: node.block.ref_into(),
        }
    }
}
impl From<&ExprAsync> for syn::ExprAsync {
    fn from(node: &ExprAsync) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            async_token: default(),
            capture: default_or_none(node.capture),
            modifiers: default(),
            block: node.block.ref_into(),
        }
    }
}

// ExprClosure: syn 3 restructured (inputs_begin/end, modifiers, removed movability)
syn_trait_impl!(syn::ExprClosure);
impl From<&syn::ExprClosure> for ExprClosure {
    fn from(node: &syn::ExprClosure) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            lifetimes: node.lifetimes.map_into(),
            constness: node.constness.is_some(),
            movability: false,
            asyncness: node.asyncness.is_some(),
            capture: node.capture.is_some(),
            inputs: node.inputs.map_into(),
            output: node.output.ref_into(),
            body: node.body.map_into(),
        }
    }
}
impl From<&ExprClosure> for syn::ExprClosure {
    fn from(node: &ExprClosure) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            lifetimes: node.lifetimes.map_into(),
            modifiers: default(),
            constness: default_or_none(node.constness),
            asyncness: default_or_none(node.asyncness),
            capture: default_or_none(node.capture),
            inputs_begin: default(),
            inputs: node.inputs.map_into(),
            inputs_end: default(),
            output: node.output.ref_into(),
            body: node.body.map_into(),
        }
    }
}

// ExprConst: syn 3 added modifiers (BlockModifiers)
syn_trait_impl!(syn::ExprConst);
impl From<&syn::ExprConst> for ExprConst {
    fn from(node: &syn::ExprConst) -> Self {
        Self { attrs: node.attrs.map_into(), block: node.block.ref_into() }
    }
}
impl From<&ExprConst> for syn::ExprConst {
    fn from(node: &ExprConst) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            const_token: default(),
            modifiers: default(),
            block: node.block.ref_into(),
        }
    }
}

// ExprTryBlock: syn 3 added modifiers (BlockModifiers)
syn_trait_impl!(syn::ExprTryBlock);
impl From<&syn::ExprTryBlock> for ExprTryBlock {
    fn from(node: &syn::ExprTryBlock) -> Self {
        Self { attrs: node.attrs.map_into(), block: node.block.ref_into() }
    }
}
impl From<&ExprTryBlock> for syn::ExprTryBlock {
    fn from(node: &ExprTryBlock) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            try_token: default(),
            modifiers: default(),
            block: node.block.ref_into(),
        }
    }
}

// Field: syn 3 changed from mutability to modifiers (FieldModifiers); no FieldMutability
// syn 3 also added a `default` field for union fields
syn_trait_impl!(syn::Field);
impl From<&syn::Field> for Field {
    fn from(node: &syn::Field) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            vis: node.vis.ref_into(),
            mutability: FieldMutability::None,
            ident: node.ident.map_into(),
            colon_token: node.colon_token.is_some(),
            ty: node.ty.ref_into(),
        }
    }
}
impl From<&Field> for syn::Field {
    fn from(node: &Field) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            vis: node.vis.ref_into(),
            modifiers: default(),
            ident: node.ident.map_into(),
            colon_token: default_or_none(node.colon_token || node.ident.is_some()),
            ty: node.ty.ref_into(),
            default: None,
        }
    }
}

// File: syn 3 added frontmatter (Frontmatter)
syn_trait_impl!(syn::File);
impl From<&syn::File> for File {
    fn from(node: &syn::File) -> Self {
        Self {
            shebang: node.shebang.clone(),
            attrs: node.attrs.map_into(),
            items: node.items.map_into(),
        }
    }
}
impl From<&File> for syn::File {
    fn from(node: &File) -> Self {
        Self {
            shebang: node.shebang.clone(),
            frontmatter: None,
            attrs: node.attrs.map_into(),
            items: node.items.map_into(),
        }
    }
}

// Local: syn 3 added modifiers (LocalModifiers)
syn_trait_impl!(syn::Local);
impl From<&syn::Local> for Local {
    fn from(node: &syn::Local) -> Self {
        Self { attrs: node.attrs.map_into(), pat: node.pat.ref_into(), init: node.init.map_into() }
    }
}
impl From<&Local> for syn::Local {
    fn from(node: &Local) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            let_token: default(),
            modifiers: default(),
            pat: node.pat.ref_into(),
            init: node.init.map_into(),
            semi_token: default(),
        }
    }
}

// Signature: syn 3 changed unsafety to safety: Safety
syn_trait_impl!(syn::Signature);
impl From<&syn::Signature> for Signature {
    fn from(node: &syn::Signature) -> Self {
        Self {
            constness: node.constness.is_some(),
            asyncness: node.asyncness.is_some(),
            unsafety: matches!(node.safety, syn::Safety::Unsafe(..)),
            abi: node.abi.map_into(),
            ident: node.ident.ref_into(),
            generics: node.generics.ref_into(),
            inputs: node.inputs.map_into(),
            variadic: node.variadic.map_into(),
            output: node.output.ref_into(),
        }
    }
}
impl From<&Signature> for syn::Signature {
    fn from(node: &Signature) -> Self {
        Self {
            constness: default_or_none(node.constness),
            asyncness: default_or_none(node.asyncness),
            safety: if node.unsafety {
                syn::Safety::Unsafe(default())
            } else {
                syn::Safety::Default
            },
            abi: node.abi.map_into(),
            fn_token: default(),
            ident: node.ident.ref_into(),
            generics: node.generics.ref_into(),
            paren_token: default(),
            inputs: node.inputs.map_into(),
            variadic: node.variadic.map_into(),
            output: node.output.ref_into(),
        }
    }
}

// ForeignItemFn: syn 3 added modifiers (FnModifiers)
syn_trait_impl!(syn::ForeignItemFn);
impl From<&syn::ForeignItemFn> for ForeignItemFn {
    fn from(node: &syn::ForeignItemFn) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            vis: node.vis.ref_into(),
            sig: node.sig.ref_into(),
        }
    }
}
impl From<&ForeignItemFn> for syn::ForeignItemFn {
    fn from(node: &ForeignItemFn) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            vis: node.vis.ref_into(),
            modifiers: default(),
            sig: node.sig.ref_into(),
            semi_token: default(),
        }
    }
}

// ForeignItemStatic: syn 3 added safety (Safety) field
syn_trait_impl!(syn::ForeignItemStatic);
impl From<&syn::ForeignItemStatic> for ForeignItemStatic {
    fn from(node: &syn::ForeignItemStatic) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            vis: node.vis.ref_into(),
            mutability: node.mutability.ref_into(),
            ident: node.ident.ref_into(),
            ty: node.ty.map_into(),
        }
    }
}
impl From<&ForeignItemStatic> for syn::ForeignItemStatic {
    fn from(node: &ForeignItemStatic) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            vis: node.vis.ref_into(),
            safety: default(),
            static_token: default(),
            mutability: node.mutability.ref_into(),
            ident: node.ident.ref_into(),
            colon_token: default(),
            ty: node.ty.map_into(),
            semi_token: default(),
        }
    }
}

// ForeignItemType: syn 3 added modifiers (TypeModifiers)
syn_trait_impl!(syn::ForeignItemType);
impl From<&syn::ForeignItemType> for ForeignItemType {
    fn from(node: &syn::ForeignItemType) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            vis: node.vis.ref_into(),
            ident: node.ident.ref_into(),
            generics: node.generics.ref_into(),
        }
    }
}
impl From<&ForeignItemType> for syn::ForeignItemType {
    fn from(node: &ForeignItemType) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            vis: node.vis.ref_into(),
            modifiers: default(),
            type_token: default(),
            ident: node.ident.ref_into(),
            generics: node.generics.ref_into(),
            semi_token: default(),
        }
    }
}

// ImplItemConst: syn 3 moved defaultness into modifiers (ConstModifiers)
syn_trait_impl!(syn::ImplItemConst);
impl From<&syn::ImplItemConst> for ImplItemConst {
    fn from(node: &syn::ImplItemConst) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            vis: node.vis.ref_into(),
            defaultness: node.modifiers.defaultness.is_some(),
            ident: node.ident.ref_into(),
            generics: node.generics.ref_into(),
            ty: node.ty.ref_into(),
            expr: node.expr.ref_into(),
        }
    }
}
impl From<&ImplItemConst> for syn::ImplItemConst {
    fn from(node: &ImplItemConst) -> Self {
        let mut modifiers = syn::ConstModifiers::default();
        if node.defaultness { modifiers.defaultness = Some(default()); }
        Self {
            attrs: node.attrs.map_into(),
            vis: node.vis.ref_into(),
            modifiers,
            const_token: default(),
            ident: node.ident.ref_into(),
            generics: node.generics.ref_into(),
            colon_token: default(),
            ty: node.ty.ref_into(),
            eq_token: default(),
            expr: node.expr.ref_into(),
            semi_token: default(),
        }
    }
}

// ImplItemFn: syn 3 moved defaultness into modifiers (FnModifiers)
syn_trait_impl!(syn::ImplItemFn);
impl From<&syn::ImplItemFn> for ImplItemFn {
    fn from(node: &syn::ImplItemFn) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            vis: node.vis.ref_into(),
            defaultness: node.modifiers.defaultness.is_some(),
            sig: node.sig.ref_into(),
            block: node.block.ref_into(),
        }
    }
}
impl From<&ImplItemFn> for syn::ImplItemFn {
    fn from(node: &ImplItemFn) -> Self {
        let mut modifiers = syn::FnModifiers::default();
        if node.defaultness { modifiers.defaultness = Some(default()); }
        Self {
            attrs: node.attrs.map_into(),
            vis: node.vis.ref_into(),
            modifiers,
            sig: node.sig.ref_into(),
            block: node.block.ref_into(),
        }
    }
}

// ImplItemType: syn 3 moved defaultness into modifiers (TypeModifiers)
syn_trait_impl!(syn::ImplItemType);
impl From<&syn::ImplItemType> for ImplItemType {
    fn from(node: &syn::ImplItemType) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            vis: node.vis.ref_into(),
            defaultness: node.modifiers.defaultness.is_some(),
            ident: node.ident.ref_into(),
            generics: node.generics.ref_into(),
            ty: node.ty.ref_into(),
        }
    }
}
impl From<&ImplItemType> for syn::ImplItemType {
    fn from(node: &ImplItemType) -> Self {
        let mut modifiers = syn::TypeModifiers::default();
        if node.defaultness { modifiers.defaultness = Some(default()); }
        Self {
            attrs: node.attrs.map_into(),
            vis: node.vis.ref_into(),
            modifiers,
            type_token: default(),
            ident: node.ident.ref_into(),
            generics: node.generics.ref_into(),
            eq_token: default(),
            ty: node.ty.ref_into(),
            semi_token: default(),
        }
    }
}

// ItemConst: syn 3 added modifiers (ConstModifiers)
syn_trait_impl!(syn::ItemConst);
impl From<&syn::ItemConst> for ItemConst {
    fn from(node: &syn::ItemConst) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            vis: node.vis.ref_into(),
            ident: node.ident.ref_into(),
            generics: node.generics.ref_into(),
            ty: node.ty.map_into(),
            expr: node.expr.map_into(),
        }
    }
}
impl From<&ItemConst> for syn::ItemConst {
    fn from(node: &ItemConst) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            vis: node.vis.ref_into(),
            modifiers: default(),
            const_token: default(),
            ident: node.ident.ref_into(),
            generics: node.generics.ref_into(),
            colon_token: default(),
            ty: node.ty.map_into(),
            eq_token: default(),
            expr: node.expr.map_into(),
            semi_token: default(),
        }
    }
}

// ItemFn: syn 3 added modifiers (FnModifiers)
syn_trait_impl!(syn::ItemFn);
impl From<&syn::ItemFn> for ItemFn {
    fn from(node: &syn::ItemFn) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            vis: node.vis.ref_into(),
            sig: node.sig.ref_into(),
            block: node.block.map_into(),
        }
    }
}
impl From<&ItemFn> for syn::ItemFn {
    fn from(node: &ItemFn) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            vis: node.vis.ref_into(),
            modifiers: default(),
            sig: node.sig.ref_into(),
            block: node.block.map_into(),
        }
    }
}

// ItemImpl: syn 3 moved defaultness/polarity into modifiers, restructured trait_
syn_trait_impl!(syn::ItemImpl);
impl From<&syn::ItemImpl> for ItemImpl {
    fn from(node: &syn::ItemImpl) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            defaultness: node.modifiers.defaultness.is_some(),
            unsafety: node.unsafety.is_some(),
            generics: node.generics.ref_into(),
            trait_: node.trait_.as_ref().map(|(path, _for)| {
                (node.modifiers.polarity.is_some(), path.ref_into())
            }),
            self_ty: node.self_ty.map_into(),
            items: node.items.map_into(),
        }
    }
}
impl From<&ItemImpl> for syn::ItemImpl {
    fn from(node: &ItemImpl) -> Self {
        let is_negative = node.trait_.as_ref().is_some_and(|(neg, _)| *neg);
        let mut modifiers = syn::ImplModifiers::default();
        if node.defaultness { modifiers.defaultness = Some(default()); }
        if is_negative { modifiers.polarity = Some(default()); }
        Self {
            attrs: node.attrs.map_into(),
            modifiers,
            unsafety: default_or_none(node.unsafety),
            impl_token: default(),
            generics: node.generics.ref_into(),
            trait_: node.trait_.as_ref().map(|(_, path)| (path.ref_into(), default())),
            self_ty: node.self_ty.map_into(),
            brace_token: default(),
            items: node.items.map_into(),
        }
    }
}

// ItemTrait: syn 3 moved auto_token into modifiers, removed restriction
syn_trait_impl!(syn::ItemTrait);
impl From<&syn::ItemTrait> for ItemTrait {
    fn from(node: &syn::ItemTrait) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            vis: node.vis.ref_into(),
            unsafety: node.unsafety.is_some(),
            auto_token: node.modifiers.auto_token.is_some(),
            restriction: None,
            ident: node.ident.ref_into(),
            generics: node.generics.ref_into(),
            colon_token: node.colon_token.is_some(),
            supertraits: node.supertraits.map_into(),
            items: node.items.map_into(),
        }
    }
}
impl From<&ItemTrait> for syn::ItemTrait {
    fn from(node: &ItemTrait) -> Self {
        let mut modifiers = syn::TraitModifiers::default();
        if node.auto_token { modifiers.auto_token = Some(default()); }
        Self {
            attrs: node.attrs.map_into(),
            vis: node.vis.ref_into(),
            modifiers,
            unsafety: default_or_none(node.unsafety),
            trait_token: default(),
            ident: node.ident.ref_into(),
            generics: node.generics.ref_into(),
            colon_token: default_or_none(node.colon_token),
            supertraits: node.supertraits.map_into(),
            brace_token: default(),
            items: node.items.map_into(),
        }
    }
}

// ItemType: syn 3 added modifiers (TypeModifiers) and where_clause_placement
syn_trait_impl!(syn::ItemType);
impl From<&syn::ItemType> for ItemType {
    fn from(node: &syn::ItemType) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            vis: node.vis.ref_into(),
            ident: node.ident.ref_into(),
            generics: node.generics.ref_into(),
            ty: node.ty.map_into(),
        }
    }
}
impl From<&ItemType> for syn::ItemType {
    fn from(node: &ItemType) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            vis: node.vis.ref_into(),
            modifiers: default(),
            type_token: default(),
            ident: node.ident.ref_into(),
            generics: node.generics.ref_into(),
            eq_token: default(),
            ty: node.ty.map_into(),
            semi_token: default(),
            where_clause_placement: syn::WhereClausePlacement::Early,
        }
    }
}

// TraitItemConst: syn 3 moved defaultness into modifiers (ConstModifiers)
syn_trait_impl!(syn::TraitItemConst);
impl From<&syn::TraitItemConst> for TraitItemConst {
    fn from(node: &syn::TraitItemConst) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            ident: node.ident.ref_into(),
            generics: node.generics.ref_into(),
            ty: node.ty.ref_into(),
            default: node.default.ref_map(|(_eq, expr)| expr.ref_into()),
        }
    }
}
impl From<&TraitItemConst> for syn::TraitItemConst {
    fn from(node: &TraitItemConst) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            modifiers: default(),
            const_token: default(),
            ident: node.ident.ref_into(),
            generics: node.generics.ref_into(),
            colon_token: default(),
            ty: node.ty.ref_into(),
            default: node.default.ref_map(|expr| (default(), expr.ref_into())),
            semi_token: default(),
        }
    }
}

// TraitItemType: syn 3 added modifiers (TypeModifiers)
syn_trait_impl!(syn::TraitItemType);
impl From<&syn::TraitItemType> for TraitItemType {
    fn from(node: &syn::TraitItemType) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            ident: node.ident.ref_into(),
            generics: node.generics.ref_into(),
            colon_token: node.colon_token.is_some(),
            bounds: node.bounds.map_into(),
            default: node.default.ref_map(|(_eq, ty)| ty.ref_into()),
        }
    }
}
impl From<&TraitItemType> for syn::TraitItemType {
    fn from(node: &TraitItemType) -> Self {
        Self {
            attrs: node.attrs.map_into(),
            modifiers: default(),
            type_token: default(),
            ident: node.ident.ref_into(),
            generics: node.generics.ref_into(),
            colon_token: default_or_none(node.colon_token),
            bounds: node.bounds.map_into(),
            default: node.default.ref_map(|ty| (default(), ty.ref_into())),
            semi_token: default(),
        }
    }
}
