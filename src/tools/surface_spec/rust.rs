//! Rust surfaces: syn syntax trees of the captured items, rendered with prettyplease.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use proc_macro2::{LineColumn, TokenStream};
use quote::quote;
use serde::{Deserialize, Serialize};
use syn::parse::{ParseStream, Parser};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::visit_mut::{self, VisitMut};
use syn::{
    AttrStyle, Attribute, ForeignItem, ImplItem, Item, Signature, Stmt, Token, TraitItem,
    Visibility,
};
use syn_serde3::Syn;

use super::{Part, Position, RecordId, Span};

#[derive(Serialize, Deserialize)]
pub(super) struct FileSurface {
    path: PathBuf,
    records: Vec<Record>,
}

impl FileSurface {
    /// The surface of `source`, which fails unless surface-spec can capture every item in it.
    pub(super) fn parse(path: PathBuf, source: &str) -> Result<Self> {
        let file =
            syn::parse_file(source).with_context(|| format!("parsing {}", path.display()))?;
        let mut collector = Collector::default();
        collector.visit_file(&file);
        if let Some(unmodeled) = collector.unmodeled {
            bail!(
                "parsing {}:{}:{}: surface-spec cannot capture this {}",
                path.display(),
                unmodeled.start.line,
                unmodeled.start.column + 1,
                unmodeled.slot,
            );
        }
        Ok(Self {
            path,
            records: collector.records,
        })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// The declarations a change adds or deletes as a whole: every record but a trait's method,
    /// which its trait holds, and a trait impl's method, which its one-line impl leaves out. An
    /// inherent impl holding methods is no part of its own, since its methods are.
    pub(super) fn parts(&self) -> Vec<Part> {
        self.records
            .iter()
            .filter(|record| match &record.node {
                Node::ImplItemFn { .. } => record.parent.is_some_and(|parent| {
                    matches!(self.records[parent.0].node, Node::InherentImpl { .. })
                }),
                Node::TraitItemFn { .. } => false,
                Node::InherentImpl { .. } => !self
                    .children(record.id)
                    .any(|child| matches!(child.node, Node::ImplItemFn { .. })),
                Node::ItemFn { .. }
                | Node::ForeignItemFn { .. }
                | Node::ItemStruct { .. }
                | Node::ItemEnum { .. }
                | Node::ItemTrait { .. }
                | Node::TraitImpl { .. } => true,
            })
            .map(|record| Part::Record(record.id))
            .collect()
    }

    /// The declarations of `parts` as Rust text in the spec's stub style, in record order, one
    /// blank line apart. An inherent impl holds only its methods among `parts`, and consecutive
    /// foreign fns of one `extern` block share a single block.
    pub(super) fn declarations(&self, parts: &BTreeSet<Part>) -> String {
        let mut groups: Vec<Group> = Vec::new();
        let shown = |record: &Record| {
            parts.contains(&Part::Record(record.id))
                || (matches!(record.node, Node::InherentImpl { .. })
                    && self
                        .children(record.id)
                        .any(|child| parts.contains(&Part::Record(child.id))))
        };
        for record in self.records.iter().filter(|record| shown(record)) {
            match (&record.node, groups.last_mut()) {
                (Node::ForeignItemFn { foreign_mod, node }, Some(Group::Extern(block, fns)))
                    if block.id == foreign_mod.id =>
                {
                    fns.push(node)
                }
                (Node::ForeignItemFn { foreign_mod, node }, _) => {
                    groups.push(Group::Extern(foreign_mod, vec![node]))
                }
                (
                    Node::ItemFn { .. }
                    | Node::ImplItemFn { .. }
                    | Node::TraitItemFn { .. }
                    | Node::ItemStruct { .. }
                    | Node::ItemEnum { .. }
                    | Node::ItemTrait { .. }
                    | Node::InherentImpl { .. }
                    | Node::TraitImpl { .. },
                    _,
                ) => groups.push(Group::Record(record)),
            }
        }
        groups
            .into_iter()
            .filter_map(|group| match group {
                Group::Extern(block, fns) => Some(block.declaration(fns)),
                Group::Record(record) => self.declaration(record, parts),
            })
            .map(|text| text.trim_end().to_owned())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// `record` as Rust text, or `None` for a method, which its impl or trait renders. An
    /// inherent impl holds only its methods among `parts`.
    fn declaration(&self, record: &Record, parts: &BTreeSet<Part>) -> Option<String> {
        let item = match &record.node {
            Node::ItemFn { node } => {
                let node = syn::ItemFn::from_adapter(node);
                Item::Verbatim(stub(&node.attrs, &node.vis, &node.sig))
            }
            Node::ForeignItemFn { foreign_mod, node } => {
                return Some(foreign_mod.declaration(vec![node]));
            }
            Node::ImplItemFn { .. } | Node::TraitItemFn { .. } => return None,
            Node::ItemStruct { node } => Item::Struct(syn::ItemStruct::from_adapter(node)),
            Node::ItemEnum { node } => Item::Enum(syn::ItemEnum::from_adapter(node)),
            Node::ItemTrait { node } => {
                let mut node = syn::ItemTrait::from_adapter(node);
                node.items.extend(
                    self.children(record.id)
                        .filter_map(|child| match &child.node {
                            Node::TraitItemFn { node } => {
                                let mut method = syn::TraitItemFn::from_adapter(node);
                                method.default = None;
                                method.semi_token = Some(Default::default());
                                Some(TraitItem::Fn(method))
                            }
                            // Children outside the trait's own items render on their own.
                            Node::ItemFn { .. }
                            | Node::ImplItemFn { .. }
                            | Node::ForeignItemFn { .. }
                            | Node::ItemStruct { .. }
                            | Node::ItemEnum { .. }
                            | Node::ItemTrait { .. }
                            | Node::InherentImpl { .. }
                            | Node::TraitImpl { .. } => None,
                        }),
                );
                Item::Trait(node)
            }
            Node::TraitImpl { node } => return Some(trait_impl(syn::ItemImpl::from_adapter(node))),
            Node::InherentImpl { node } => {
                let mut node = syn::ItemImpl::from_adapter(node);
                node.items = self
                    .children(record.id)
                    .filter(|child| parts.contains(&Part::Record(child.id)))
                    .filter_map(|child| match &child.node {
                        Node::ImplItemFn { node } => {
                            let method = syn::ImplItemFn::from_adapter(node);
                            let attrs = &method.attrs;
                            let defaultness = method.modifiers.defaultness;
                            let signature = stub(&[], &method.vis, &method.sig);
                            Some(ImplItem::Verbatim(
                                quote! { #(#attrs)* #defaultness #signature },
                            ))
                        }
                        // Children outside the impl's own items render on their own.
                        Node::ItemFn { .. }
                        | Node::TraitItemFn { .. }
                        | Node::ForeignItemFn { .. }
                        | Node::ItemStruct { .. }
                        | Node::ItemEnum { .. }
                        | Node::ItemTrait { .. }
                        | Node::InherentImpl { .. }
                        | Node::TraitImpl { .. } => None,
                    })
                    .collect();
                Item::Impl(node)
            }
        };
        Some(unparse(item))
    }

    /// The records whose innermost enclosing record is `id`.
    fn children(&self, id: RecordId) -> impl Iterator<Item = &Record> {
        self.records
            .iter()
            .filter(move |record| record.parent == Some(id))
    }
}

#[derive(Serialize, Deserialize)]
struct Record {
    id: RecordId,
    parent: Option<RecordId>,
    mod_path: Vec<String>,
    kind: Kind,
    span: Span,
    #[serde(flatten)]
    node: Node,
}

/// Records that render as one declaration.
enum Group<'a> {
    Record(&'a Record),
    /// Consecutive foreign fns of one `extern` block.
    Extern(
        &'a ForeignMod,
        Vec<&'a <syn::ForeignItemFn as Syn>::Adapter>,
    ),
}

/// An `extern` block, which is not a captured kind of its own.
#[derive(Serialize, Deserialize)]
struct ForeignMod {
    id: ForeignModId,
    #[serde(rename = "unsafe")]
    unsafety: bool,
    abi: <syn::Abi as Syn>::Adapter,
}

impl ForeignMod {
    fn new(id: ForeignModId, block: &syn::ItemForeignMod) -> Self {
        Self {
            id,
            unsafety: block.unsafety.is_some(),
            abi: block.abi.to_adapter(),
        }
    }

    /// The block holding the stubs of `fns`, which must all come from this block.
    fn declaration(&self, fns: Vec<&<syn::ForeignItemFn as Syn>::Adapter>) -> String {
        let items = fns
            .into_iter()
            .map(|node| ForeignItem::Fn(syn::ForeignItemFn::from_adapter(node)))
            .collect();
        unparse(Item::ForeignMod(syn::ItemForeignMod {
            attrs: Vec::new(),
            unsafety: self.unsafety.then(Default::default),
            abi: syn::Abi::from_adapter(&self.abi),
            brace_token: Default::default(),
            items,
        }))
    }
}

/// Position of an `extern` block in its file's walk order.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct ForeignModId(usize);

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Fn,
    Struct,
    Enum,
    Trait,
    Impl,
}

/// A captured node after pruning, as syn-serde serializes it, beside the name of its syn type,
/// which tells apart node types that serialize alike.
#[derive(Serialize, Deserialize)]
#[serde(tag = "syn")]
enum Node {
    ItemFn {
        node: <syn::ItemFn as Syn>::Adapter,
    },
    ImplItemFn {
        node: <syn::ImplItemFn as Syn>::Adapter,
    },
    TraitItemFn {
        node: <syn::TraitItemFn as Syn>::Adapter,
    },
    ForeignItemFn {
        /// The `extern` block enclosing the fn.
        foreign_mod: ForeignMod,
        node: <syn::ForeignItemFn as Syn>::Adapter,
    },
    ItemStruct {
        node: <syn::ItemStruct as Syn>::Adapter,
    },
    ItemEnum {
        node: <syn::ItemEnum as Syn>::Adapter,
    },
    ItemTrait {
        node: <syn::ItemTrait as Syn>::Adapter,
    },
    InherentImpl {
        node: <syn::ItemImpl as Syn>::Adapter,
    },
    TraitImpl {
        node: <syn::ItemImpl as Syn>::Adapter,
    },
}

impl Node {
    fn kind(&self) -> Kind {
        match self {
            Node::ItemFn { .. }
            | Node::ImplItemFn { .. }
            | Node::TraitItemFn { .. }
            | Node::ForeignItemFn { .. } => Kind::Fn,
            Node::ItemStruct { .. } => Kind::Struct,
            Node::ItemEnum { .. } => Kind::Enum,
            Node::ItemTrait { .. } => Kind::Trait,
            Node::InherentImpl { .. } | Node::TraitImpl { .. } => Kind::Impl,
        }
    }
}

impl From<proc_macro2::Span> for Span {
    fn from(span: proc_macro2::Span) -> Self {
        Self {
            start: span.start().into(),
            end: span.end().into(),
        }
    }
}

impl From<LineColumn> for Position {
    fn from(location: LineColumn) -> Self {
        Self {
            line: location.line,
            column: location.column,
        }
    }
}

fn stub(attrs: &[Attribute], vis: &Visibility, sig: &Signature) -> TokenStream {
    quote! { #(#attrs)* #vis #sig; }
}

/// `impl Trait for Type;`, the spec's form of a trait implementation, which syn cannot print.
fn trait_impl(mut node: syn::ItemImpl) -> String {
    node.items.clear();
    let text = unparse(Item::Impl(node));
    let header = text
        .strip_suffix("{}\n")
        .expect("prettyplease prints an empty impl body as `{}`");
    format!("{};", header.trim_end_matches(' '))
}

fn unparse(item: Item) -> String {
    prettyplease::unparse(&syn::File {
        shebang: None,
        frontmatter: None,
        attrs: Vec::new(),
        items: vec![item],
    })
}

/// Walks every node of a file, recording each captured kind.
#[derive(Default)]
struct Collector {
    records: Vec<Record>,
    /// Captured records enclosing the current node, innermost last.
    parents: Vec<RecordId>,
    mod_path: Vec<String>,
    /// The `extern` blocks walked so far.
    foreign_mods: usize,
    /// The first node surface-spec cannot capture.
    unmodeled: Option<Unmodeled>,
}

/// A node surface-spec cannot capture, such as syn's verbatim tokens for syntax newer than syn, which
/// may hide captured kinds.
struct Unmodeled {
    slot: Slot,
    start: Position,
}

/// The syn enums whose variants include the captured kinds.
#[derive(Clone, Copy)]
enum Slot {
    Item,
    ImplItem,
    TraitItem,
    ForeignItem,
}

impl fmt::Display for Slot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Slot::Item => "item",
            Slot::ImplItem => "impl item",
            Slot::TraitItem => "trait item",
            Slot::ForeignItem => "foreign item",
        })
    }
}

impl Collector {
    fn unmodeled(&mut self, slot: Slot, span: proc_macro2::Span) {
        self.unmodeled.get_or_insert_with(|| Unmodeled {
            slot,
            start: span.start().into(),
        });
    }

    fn capture(&mut self, span: proc_macro2::Span, node: Node, descend: impl FnOnce(&mut Self)) {
        let id = RecordId(self.records.len());
        self.records.push(Record {
            id,
            parent: self.parents.last().copied(),
            mod_path: self.mod_path.clone(),
            kind: node.kind(),
            span: span.into(),
            node,
        });
        self.parents.push(id);
        descend(self);
        self.parents.pop();
    }

    fn capture_impl_item_fn(&mut self, span: proc_macro2::Span, node: &syn::ImplItemFn) {
        let pruned = Node::ImplItemFn {
            node: pruned(node, Prune::visit_impl_item_fn_mut),
        };
        self.capture(span, pruned, |collector| {
            visit::visit_impl_item_fn(collector, node)
        });
    }

    fn capture_foreign_fn(&mut self, foreign_mod: ForeignMod, node: &syn::ForeignItemFn) {
        // The capture's signature records only `unsafe`, so `safe` would vanish from the spec.
        if let syn::Safety::Safe(_) = node.sig.safety {
            self.unmodeled(Slot::ForeignItem, node.span());
        }
        let pruned = Node::ForeignItemFn {
            foreign_mod,
            node: pruned(node, Prune::visit_foreign_item_fn_mut),
        };
        self.capture(node.span(), pruned, |collector| {
            visit::visit_foreign_item_fn(collector, node)
        });
    }
}

// Each variant syn models is listed, so verbatim tokens other than a bodiless method and any
// variant added after this code fail the file instead of passing unwalked.
impl<'ast> Visit<'ast> for Collector {
    fn visit_item(&mut self, node: &'ast Item) {
        match node {
            Item::Const(_)
            | Item::Enum(_)
            | Item::ExternCrate(_)
            | Item::Fn(_)
            | Item::ForeignMod(_)
            | Item::Impl(_)
            | Item::Macro(_)
            | Item::Mod(_)
            | Item::Static(_)
            | Item::Struct(_)
            | Item::Trait(_)
            | Item::TraitAlias(_)
            | Item::Type(_)
            | Item::Union(_)
            | Item::Use(_) => visit::visit_item(self, node),
            _ => self.unmodeled(Slot::Item, node.span()),
        }
    }

    fn visit_impl_item(&mut self, node: &'ast ImplItem) {
        match node {
            ImplItem::Const(_) | ImplItem::Fn(_) | ImplItem::Type(_) | ImplItem::Macro(_) => {
                visit::visit_impl_item(self, node)
            }
            ImplItem::Verbatim(tokens) => match bodiless_fn(tokens) {
                Some(method) => self.capture_impl_item_fn(node.span(), &method),
                None => self.unmodeled(Slot::ImplItem, node.span()),
            },
            _ => self.unmodeled(Slot::ImplItem, node.span()),
        }
    }

    fn visit_trait_item(&mut self, node: &'ast TraitItem) {
        match node {
            TraitItem::Const(_) | TraitItem::Fn(_) | TraitItem::Type(_) | TraitItem::Macro(_) => {
                visit::visit_trait_item(self, node)
            }
            _ => self.unmodeled(Slot::TraitItem, node.span()),
        }
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        self.mod_path.push(node.ident.to_string());
        visit::visit_item_mod(self, node);
        self.mod_path.pop();
    }

    // The block's items are walked here, where each foreign fn can carry its block.
    fn visit_item_foreign_mod(&mut self, node: &'ast syn::ItemForeignMod) {
        let id = ForeignModId(self.foreign_mods);
        self.foreign_mods += 1;
        for item in &node.items {
            match item {
                ForeignItem::Fn(item) => self.capture_foreign_fn(ForeignMod::new(id, node), item),
                ForeignItem::Static(_) | ForeignItem::Type(_) | ForeignItem::Macro(_) => {
                    visit::visit_foreign_item(self, item)
                }
                _ => self.unmodeled(Slot::ForeignItem, item.span()),
            }
        }
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let pruned = Node::ItemFn {
            node: pruned(node, Prune::visit_item_fn_mut),
        };
        self.capture(node.span(), pruned, |collector| {
            visit::visit_item_fn(collector, node)
        });
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        self.capture_impl_item_fn(node.span(), node);
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        let pruned = Node::TraitItemFn {
            node: pruned(node, Prune::visit_trait_item_fn_mut),
        };
        self.capture(node.span(), pruned, |collector| {
            visit::visit_trait_item_fn(collector, node)
        });
    }

    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        let pruned = Node::ItemStruct {
            node: pruned(node, Prune::visit_item_struct_mut),
        };
        self.capture(node.span(), pruned, |collector| {
            visit::visit_item_struct(collector, node)
        });
    }

    fn visit_item_enum(&mut self, node: &'ast syn::ItemEnum) {
        let pruned = Node::ItemEnum {
            node: pruned(node, Prune::visit_item_enum_mut),
        };
        self.capture(node.span(), pruned, |collector| {
            visit::visit_item_enum(collector, node)
        });
    }

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        let pruned = Node::ItemTrait {
            node: pruned(node, Prune::visit_item_trait_mut),
        };
        self.capture(node.span(), pruned, |collector| {
            visit::visit_item_trait(collector, node)
        });
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        let pruned = pruned(node, Prune::visit_item_impl_mut);
        let pruned = if node.trait_.is_some() {
            Node::TraitImpl { node: pruned }
        } else {
            Node::InherentImpl { node: pruned }
        };
        self.capture(node.span(), pruned, |collector| {
            visit::visit_item_impl(collector, node)
        });
    }
}

/// The method in `tokens` when they are an impl fn without a body, which syn parses but keeps
/// only as verbatim tokens, given an empty body.
fn bodiless_fn(tokens: &TokenStream) -> Option<syn::ImplItemFn> {
    parse_bodiless_fn.parse2(tokens.clone()).ok()
}

/// syn's grammar for an impl fn without a body: `Attribute* Visibility default? Signature ;`.
fn parse_bodiless_fn(input: ParseStream) -> syn::Result<syn::ImplItemFn> {
    let attrs = input.call(Attribute::parse_outer)?;
    let vis = input.parse()?;
    let mut modifiers = syn::FnModifiers::default();
    modifiers.defaultness = input.parse()?;
    let sig = input.parse()?;
    input.parse::<Token![;]>()?;
    Ok(syn::ImplItemFn {
        attrs,
        vis,
        modifiers,
        sig,
        block: syn::Block {
            brace_token: Default::default(),
            stmts: Vec::new(),
        },
    })
}

/// A clone of `node` with `prune` applied, as syn-serde serializes it.
fn pruned<N: Clone + Syn>(node: &N, prune: fn(&mut Prune, &mut N)) -> N::Adapter {
    let mut node = node.clone();
    prune(&mut Prune, &mut node);
    node.to_adapter()
}

/// Strips a captured node down to its own definition: no doc comments or inner attributes, empty
/// function bodies, and no captured kinds nested inside it, since those are records of their own.
struct Prune;

/// Drops doc comments and inner attributes, keeping every other attribute.
fn strip_attrs(attrs: &mut Vec<Attribute>) {
    attrs.retain(|attr| matches!(attr.style, AttrStyle::Outer) && !attr.path().is_ident("doc"));
}

/// Overrides that strip the attributes of a syn node before walking into it.
macro_rules! strip_attrs {
    ($($visit:ident: $ty:ident),* $(,)?) => {
        $(fn $visit(&mut self, node: &mut syn::$ty) {
            strip_attrs(&mut node.attrs);
            visit_mut::$visit(self, node);
        })*
    };
}

impl VisitMut for Prune {
    strip_attrs! {
        visit_arm_mut: Arm,
        visit_named_arg_mut: NamedArg,
        visit_fn_ptr_variadic_mut: FnPtrVariadic,
        visit_const_param_mut: ConstParam,
        visit_expr_array_mut: ExprArray,
        visit_expr_assign_mut: ExprAssign,
        visit_expr_async_mut: ExprAsync,
        visit_expr_await_mut: ExprAwait,
        visit_expr_binary_mut: ExprBinary,
        visit_expr_block_mut: ExprBlock,
        visit_expr_break_mut: ExprBreak,
        visit_expr_call_mut: ExprCall,
        visit_expr_cast_mut: ExprCast,
        visit_expr_closure_mut: ExprClosure,
        visit_expr_const_mut: ExprConst,
        visit_expr_continue_mut: ExprContinue,
        visit_expr_field_mut: ExprField,
        visit_expr_for_loop_mut: ExprForLoop,
        visit_expr_group_mut: ExprGroup,
        visit_expr_if_mut: ExprIf,
        visit_expr_index_mut: ExprIndex,
        visit_expr_infer_mut: ExprInfer,
        visit_expr_let_mut: ExprLet,
        visit_expr_lit_mut: ExprLit,
        visit_expr_loop_mut: ExprLoop,
        visit_expr_macro_mut: ExprMacro,
        visit_expr_match_mut: ExprMatch,
        visit_expr_method_call_mut: ExprMethodCall,
        visit_expr_paren_mut: ExprParen,
        visit_expr_path_mut: ExprPath,
        visit_expr_range_mut: ExprRange,
        visit_expr_raw_addr_mut: ExprRawAddr,
        visit_expr_reference_mut: ExprReference,
        visit_expr_repeat_mut: ExprRepeat,
        visit_expr_return_mut: ExprReturn,
        visit_expr_struct_mut: ExprStruct,
        visit_expr_try_mut: ExprTry,
        visit_expr_try_block_mut: ExprTryBlock,
        visit_expr_tuple_mut: ExprTuple,
        visit_expr_unary_mut: ExprUnary,
        visit_expr_unsafe_mut: ExprUnsafe,
        visit_expr_while_mut: ExprWhile,
        visit_expr_yield_mut: ExprYield,
        visit_field_mut: Field,
        visit_field_pat_mut: FieldPat,
        visit_field_value_mut: FieldValue,
        visit_foreign_item_macro_mut: ForeignItemMacro,
        visit_foreign_item_static_mut: ForeignItemStatic,
        visit_foreign_item_type_mut: ForeignItemType,
        visit_impl_item_const_mut: ImplItemConst,
        visit_impl_item_macro_mut: ImplItemMacro,
        visit_impl_item_type_mut: ImplItemType,
        visit_item_const_mut: ItemConst,
        visit_item_enum_mut: ItemEnum,
        visit_item_extern_crate_mut: ItemExternCrate,
        visit_item_macro_mut: ItemMacro,
        visit_item_static_mut: ItemStatic,
        visit_item_struct_mut: ItemStruct,
        visit_item_trait_alias_mut: ItemTraitAlias,
        visit_item_type_mut: ItemType,
        visit_item_union_mut: ItemUnion,
        visit_item_use_mut: ItemUse,
        visit_lifetime_param_mut: LifetimeParam,
        visit_local_mut: Local,
        visit_pat_ident_mut: PatIdent,
        visit_pat_or_mut: PatOr,
        visit_pat_paren_mut: PatParen,
        visit_pat_reference_mut: PatReference,
        visit_pat_rest_mut: PatRest,
        visit_pat_slice_mut: PatSlice,
        visit_pat_struct_mut: PatStruct,
        visit_pat_tuple_mut: PatTuple,
        visit_pat_tuple_struct_mut: PatTupleStruct,
        visit_pat_type_mut: PatType,
        visit_pat_wild_mut: PatWild,
        visit_receiver_mut: Receiver,
        visit_stmt_macro_mut: StmtMacro,
        visit_trait_item_const_mut: TraitItemConst,
        visit_trait_item_macro_mut: TraitItemMacro,
        visit_trait_item_type_mut: TraitItemType,
        visit_type_param_mut: TypeParam,
        visit_variadic_mut: Variadic,
        visit_variant_mut: Variant,
    }

    fn visit_lit_mut(&mut self, node: &mut syn::Lit) {
        // syn-serde serializes a byte literal as `52u8` but deserializes it expecting `b'4'`, so
        // it keeps the literal's own tokens instead.
        if let syn::Lit::Byte(byte) = node {
            *node = syn::Lit::Verbatim(byte.token());
        }
    }

    fn visit_item_fn_mut(&mut self, node: &mut syn::ItemFn) {
        strip_attrs(&mut node.attrs);
        node.block.stmts.clear();
        visit_mut::visit_item_fn_mut(self, node);
    }

    fn visit_impl_item_fn_mut(&mut self, node: &mut syn::ImplItemFn) {
        strip_attrs(&mut node.attrs);
        node.block.stmts.clear();
        visit_mut::visit_impl_item_fn_mut(self, node);
    }

    fn visit_trait_item_fn_mut(&mut self, node: &mut syn::TraitItemFn) {
        strip_attrs(&mut node.attrs);
        if let Some(block) = &mut node.default {
            block.stmts.clear();
        }
        visit_mut::visit_trait_item_fn_mut(self, node);
    }

    fn visit_foreign_item_fn_mut(&mut self, node: &mut syn::ForeignItemFn) {
        strip_attrs(&mut node.attrs);
        visit_mut::visit_foreign_item_fn_mut(self, node);
    }

    fn visit_item_impl_mut(&mut self, node: &mut syn::ItemImpl) {
        strip_attrs(&mut node.attrs);
        node.items.retain(|item| match item {
            ImplItem::Fn(_) => false,
            ImplItem::Verbatim(tokens) => bodiless_fn(tokens).is_none(),
            _ => true,
        });
        visit_mut::visit_item_impl_mut(self, node);
    }

    fn visit_item_trait_mut(&mut self, node: &mut syn::ItemTrait) {
        strip_attrs(&mut node.attrs);
        node.items.retain(|item| !matches!(item, TraitItem::Fn(_)));
        visit_mut::visit_item_trait_mut(self, node);
    }

    fn visit_item_foreign_mod_mut(&mut self, node: &mut syn::ItemForeignMod) {
        strip_attrs(&mut node.attrs);
        node.items
            .retain(|item| !matches!(item, ForeignItem::Fn(_)));
        visit_mut::visit_item_foreign_mod_mut(self, node);
    }

    fn visit_item_mod_mut(&mut self, node: &mut syn::ItemMod) {
        strip_attrs(&mut node.attrs);
        if let Some((_, items)) = &mut node.content {
            items.retain(|item| !captured(item));
        }
        visit_mut::visit_item_mod_mut(self, node);
    }

    // Blocks outside function bodies, such as array lengths, enum discriminants, and const
    // initializers, can still declare items.
    fn visit_block_mut(&mut self, node: &mut syn::Block) {
        node.stmts
            .retain(|stmt| !matches!(stmt, Stmt::Item(item) if captured(item)));
        visit_mut::visit_block_mut(self, node);
    }
}

fn captured(item: &Item) -> bool {
    matches!(
        item,
        Item::Fn(_) | Item::Struct(_) | Item::Enum(_) | Item::Trait(_) | Item::Impl(_)
    )
}
