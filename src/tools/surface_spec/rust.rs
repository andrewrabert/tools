//! Rust surfaces: syn syntax trees of the captured items, rendered with prettyplease.

use std::collections::BTreeSet;
use std::fmt;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::Once;

use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};
use proc_macro2::{LineColumn, TokenStream};
use quote::quote;
use serde::{Deserialize, Serialize};
use syn::parse::{ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::visit_mut::{self, VisitMut};
use syn::{
    AttrStyle, Attribute, ForeignItem, ImplItem, Item, Meta, Signature, Stmt, Token, TraitItem,
    Visibility,
};
use syn_serde3::Syn;

use super::{Justification, NodeId, Position, Span};

/// The Rust items a spec includes, by their visibility and by whether they exist only in test
/// builds.
#[derive(Args, Default)]
pub(super) struct Includes {
    /// Include Rust items limited to their own module: with no visibility, `pub(self)`, or
    /// `pub(in self)`
    #[arg(
        id = "include_rust_private",
        long = "include-rust-private",
        value_enum,
        default_value_t
    )]
    private: Include,
    /// Include Rust items that exist only in test builds, such as `#[cfg(test)]` modules and
    /// `#[test]` functions
    #[arg(
        id = "include_rust_tests",
        long = "include-rust-tests",
        value_enum,
        default_value_t
    )]
    tests: Include,
}

impl Includes {
    fn keeps(&self, file: &FileSurface, node: &Node) -> bool {
        self.private.keeps(file.private(node)) && self.tests.keeps(node.test)
    }
}

#[derive(Clone, Copy, Default, ValueEnum)]
enum Include {
    /// Include them with the other items
    #[default]
    Yes,
    /// Leave them out
    No,
    /// Include them and leave out the other items
    Only,
}

impl Include {
    /// Whether an item is included, given whether it is one of the items this includes.
    fn keeps(self, applies: bool) -> bool {
        match self {
            Self::Yes => true,
            Self::No => !applies,
            Self::Only => applies,
        }
    }
}

pub(super) struct FileSurface {
    path: PathBuf,
    nodes: Vec<Node>,
}

impl FileSurface {
    /// The surface of `source`, which fails unless surface-spec can capture every item in it.
    pub(super) fn parse(path: PathBuf, source: &str) -> Result<Self> {
        let file =
            syn::parse_file(source).with_context(|| format!("parsing {}", path.display()))?;
        silence_syn_serde_panics();
        let mut collector = Collector::default();
        collector.within(test_only(&file.attrs), |collector| {
            collector.visit_file(&file)
        });
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
            nodes: collector.nodes,
        })
    }

    /// The surface of the file at `path` holding `nodes`, in id order, each after its parent.
    pub(super) fn new(path: PathBuf, nodes: Vec<Node>) -> Self {
        Self { path, nodes }
    }

    pub(super) fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The declarations a change adds or deletes as a whole, among the nodes `includes` keeps:
    /// every node but a trait's method, which its trait holds, a trait impl's method, which
    /// its one-line impl leaves out, and a field or variant, which its struct or enum holds. An
    /// inherent impl holding methods is no part of its own, since its methods are.
    pub(super) fn parts(&self, includes: &Includes) -> Vec<NodeId> {
        self.nodes
            .iter()
            .filter(|node| includes.keeps(self, node))
            .filter(|node| match &node.syntax {
                Syntax::ImplItemFn { .. } => node.parent.is_some_and(|parent| {
                    matches!(self.node(parent).syntax, Syntax::InherentImpl { .. })
                }),
                Syntax::TraitItemFn { .. } | Syntax::Field { .. } | Syntax::Variant { .. } => false,
                Syntax::InherentImpl { .. } => !self
                    .children(node.id)
                    .any(|child| matches!(child.syntax, Syntax::ImplItemFn { .. })),
                Syntax::ItemFn { .. }
                | Syntax::ForeignItemFn { .. }
                | Syntax::ItemStruct { .. }
                | Syntax::ItemEnum { .. }
                | Syntax::ItemTrait { .. }
                | Syntax::TraitImpl { .. } => true,
            })
            .map(|node| node.id)
            .collect()
    }

    /// The declarations of `parts` as Rust text in the spec's stub style, in node order, one
    /// blank line apart. An inherent impl holds only its methods among `parts`, and consecutive
    /// foreign fns of one `extern` block share a single block.
    pub(super) fn declarations(&self, parts: &BTreeSet<NodeId>) -> String {
        let mut groups: Vec<Group> = Vec::new();
        let shown = |node: &Node| {
            parts.contains(&node.id)
                || (matches!(node.syntax, Syntax::InherentImpl { .. })
                    && self
                        .children(node.id)
                        .any(|child| parts.contains(&child.id)))
        };
        for node in self.nodes.iter().filter(|node| shown(node)) {
            match (&node.syntax, groups.last_mut()) {
                (Syntax::ForeignItemFn { foreign_mod, .. }, Some(Group::Extern(block, fns)))
                    if block.id == foreign_mod.id =>
                {
                    fns.push(node)
                }
                (Syntax::ForeignItemFn { foreign_mod, .. }, _) => {
                    groups.push(Group::Extern(foreign_mod, vec![node]))
                }
                (
                    Syntax::ItemFn { .. }
                    | Syntax::ImplItemFn { .. }
                    | Syntax::TraitItemFn { .. }
                    | Syntax::Field { .. }
                    | Syntax::Variant { .. }
                    | Syntax::ItemStruct { .. }
                    | Syntax::ItemEnum { .. }
                    | Syntax::ItemTrait { .. }
                    | Syntax::InherentImpl { .. }
                    | Syntax::TraitImpl { .. },
                    _,
                ) => groups.push(Group::Node(node)),
            }
        }
        groups
            .into_iter()
            .filter_map(|group| match group {
                Group::Extern(block, fns) => Some(block.declaration(fns)),
                Group::Node(node) => self.declaration(node, Holding::Parts(parts)),
            })
            .map(|text| text.trim_end().to_owned())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// The declaration of node `id` inside the declarations enclosing it that render it, each
    /// holding only the nodes on the way to it, and holding every node it renders but a trait
    /// impl's methods.
    pub(super) fn surface(&self, id: NodeId) -> String {
        let target = self.node(id);
        let mut held = BTreeSet::from([id]);
        if !matches!(target.syntax, Syntax::TraitImpl { .. }) {
            held.extend(self.descendants(id));
        }
        let mut top = target;
        while let Some(parent) = top.parent.map(|parent| self.node(parent))
            && renders(parent, top)
        {
            held.insert(parent.id);
            top = parent;
        }
        self.declaration(top, Holding::Only(&held))
            .expect("a method, field, or variant climbs to the node rendering it")
    }

    /// `node` as Rust text, or `None` for a method, field, or variant, which its impl, trait,
    /// struct, or enum renders, holding the children `holding` names.
    fn declaration(&self, node: &Node, holding: Holding) -> Option<String> {
        let held = |child: &&Node| holding.holds(node, child);
        let item = match &node.syntax {
            Syntax::ItemFn { syntax } => {
                let mut syntax = syn::ItemFn::from_adapter(syntax);
                justify(&mut syntax.attrs, node);
                Item::Verbatim(stub(&syntax.attrs, &syntax.vis, &syntax.sig))
            }
            Syntax::ForeignItemFn { foreign_mod, .. } => {
                return Some(foreign_mod.declaration(vec![node]));
            }
            Syntax::ImplItemFn { .. }
            | Syntax::TraitItemFn { .. }
            | Syntax::Field { .. }
            | Syntax::Variant { .. } => return None,
            Syntax::ItemStruct { syntax } => {
                let mut syntax = syn::ItemStruct::from_adapter(syntax);
                justify(&mut syntax.attrs, node);
                self.fill_fields(&mut syntax.fields, node, holding);
                Item::Struct(syntax)
            }
            Syntax::ItemEnum { syntax } => {
                let mut syntax = syn::ItemEnum::from_adapter(syntax);
                justify(&mut syntax.attrs, node);
                syntax
                    .variants
                    .extend(self.children(node.id).filter(held).filter_map(|child| {
                        match &child.syntax {
                            Syntax::Variant { syntax } => {
                                let mut variant = syn::Variant::from_adapter(syntax);
                                justify(&mut variant.attrs, child);
                                self.fill_fields(&mut variant.fields, child, holding);
                                Some(variant)
                            }
                            // Children outside the enum's own variants render on their own.
                            Syntax::ItemFn { .. }
                            | Syntax::ImplItemFn { .. }
                            | Syntax::TraitItemFn { .. }
                            | Syntax::ForeignItemFn { .. }
                            | Syntax::Field { .. }
                            | Syntax::ItemStruct { .. }
                            | Syntax::ItemEnum { .. }
                            | Syntax::ItemTrait { .. }
                            | Syntax::InherentImpl { .. }
                            | Syntax::TraitImpl { .. } => None,
                        }
                    }));
                Item::Enum(syntax)
            }
            Syntax::ItemTrait { syntax } => {
                let mut syntax = syn::ItemTrait::from_adapter(syntax);
                justify(&mut syntax.attrs, node);
                syntax
                    .items
                    .extend(self.children(node.id).filter(held).filter_map(|child| {
                        match &child.syntax {
                            Syntax::TraitItemFn { syntax } => {
                                let mut method = syn::TraitItemFn::from_adapter(syntax);
                                justify(&mut method.attrs, child);
                                method.default = None;
                                method.semi_token = Some(Default::default());
                                Some(TraitItem::Fn(method))
                            }
                            // Children outside the trait's own items render on their own.
                            Syntax::ItemFn { .. }
                            | Syntax::ImplItemFn { .. }
                            | Syntax::ForeignItemFn { .. }
                            | Syntax::Field { .. }
                            | Syntax::Variant { .. }
                            | Syntax::ItemStruct { .. }
                            | Syntax::ItemEnum { .. }
                            | Syntax::ItemTrait { .. }
                            | Syntax::InherentImpl { .. }
                            | Syntax::TraitImpl { .. } => None,
                        }
                    }));
                Item::Trait(syntax)
            }
            Syntax::TraitImpl { syntax } | Syntax::InherentImpl { syntax } => {
                let mut syntax = syn::ItemImpl::from_adapter(syntax);
                justify(&mut syntax.attrs, node);
                syntax.items = self
                    .children(node.id)
                    .filter(held)
                    .filter_map(|child| match &child.syntax {
                        Syntax::ImplItemFn { syntax } => {
                            let mut method = syn::ImplItemFn::from_adapter(syntax);
                            justify(&mut method.attrs, child);
                            let attrs = &method.attrs;
                            let defaultness = method.modifiers.defaultness;
                            let signature = stub(&[], &method.vis, &method.sig);
                            Some(ImplItem::Verbatim(
                                quote! { #(#attrs)* #defaultness #signature },
                            ))
                        }
                        // Children outside the impl's own items render on their own.
                        Syntax::ItemFn { .. }
                        | Syntax::TraitItemFn { .. }
                        | Syntax::ForeignItemFn { .. }
                        | Syntax::Field { .. }
                        | Syntax::Variant { .. }
                        | Syntax::ItemStruct { .. }
                        | Syntax::ItemEnum { .. }
                        | Syntax::ItemTrait { .. }
                        | Syntax::InherentImpl { .. }
                        | Syntax::TraitImpl { .. } => None,
                    })
                    .collect();
                if matches!(node.syntax, Syntax::TraitImpl { .. }) && syntax.items.is_empty() {
                    return Some(trait_impl(syntax));
                }
                Item::Impl(syntax)
            }
        };
        Some(unparse(item))
    }

    /// Whether `node` is limited to its own module: a trait's method and an enum's variant by
    /// their trait or enum, and an impl, which has no visibility of its own, never.
    fn private(&self, node: &Node) -> bool {
        let vis = match &node.syntax {
            Syntax::ItemFn { syntax } => syn::ItemFn::from_adapter(syntax).vis,
            Syntax::ImplItemFn { syntax } => syn::ImplItemFn::from_adapter(syntax).vis,
            Syntax::ForeignItemFn { syntax, .. } => syn::ForeignItemFn::from_adapter(syntax).vis,
            Syntax::ItemStruct { syntax } => syn::ItemStruct::from_adapter(syntax).vis,
            Syntax::ItemEnum { syntax } => syn::ItemEnum::from_adapter(syntax).vis,
            Syntax::ItemTrait { syntax } => syn::ItemTrait::from_adapter(syntax).vis,
            Syntax::Field { syntax } => syn::Field::from_adapter(syntax).vis,
            Syntax::TraitItemFn { .. } | Syntax::Variant { .. } => {
                return node
                    .parent
                    .is_some_and(|parent| self.private(self.node(parent)));
            }
            Syntax::InherentImpl { .. } | Syntax::TraitImpl { .. } => return false,
        };
        private(&vis)
    }

    /// Fills `fields`, which holds none, with the fields among the children of `parent` that
    /// `holding` names.
    fn fill_fields(&self, fields: &mut syn::Fields, parent: &Node, holding: Holding) {
        let children = self
            .children(parent.id)
            .filter(|child| holding.holds(parent, child))
            .filter_map(|child| match &child.syntax {
                Syntax::Field { syntax } => {
                    let mut field = syn::Field::from_adapter(syntax);
                    justify(&mut field.attrs, child);
                    Some(field)
                }
                // Children outside the fields render on their own.
                Syntax::ItemFn { .. }
                | Syntax::ImplItemFn { .. }
                | Syntax::TraitItemFn { .. }
                | Syntax::ForeignItemFn { .. }
                | Syntax::Variant { .. }
                | Syntax::ItemStruct { .. }
                | Syntax::ItemEnum { .. }
                | Syntax::ItemTrait { .. }
                | Syntax::InherentImpl { .. }
                | Syntax::TraitImpl { .. } => None,
            });
        match fields {
            syn::Fields::Named(fields) => fields.named.extend(children),
            syn::Fields::Unnamed(fields) => fields.unnamed.extend(children),
            syn::Fields::Unit => {}
        }
    }

    /// The node `id` names, which the file holds.
    fn node(&self, id: NodeId) -> &Node {
        let index = self
            .nodes
            .binary_search_by_key(&id, |node| node.id)
            .expect("a file's nodes are in id order and hold their parents");
        &self.nodes[index]
    }

    fn children(&self, id: NodeId) -> impl Iterator<Item = &Node> {
        self.nodes
            .iter()
            .filter(move |node| node.parent == Some(id))
    }

    /// The nodes inside node `id`, at any depth.
    fn descendants(&self, id: NodeId) -> BTreeSet<NodeId> {
        let mut descendants = BTreeSet::new();
        // Nodes in id order follow their parents.
        for node in &self.nodes {
            if node
                .parent
                .is_some_and(|parent| parent == id || descendants.contains(&parent))
            {
                descendants.insert(node.id);
            }
        }
        descendants
    }
}

/// Which of a node's children its declaration holds.
#[derive(Clone, Copy)]
enum Holding<'a> {
    /// The spec's: the methods among these parts for an inherent impl, none for a trait impl's
    /// one line, and every field, variant, or method for any other node.
    Parts(&'a BTreeSet<NodeId>),
    /// The children among these nodes, for any node.
    Only(&'a BTreeSet<NodeId>),
}

impl Holding<'_> {
    fn holds(self, parent: &Node, child: &Node) -> bool {
        match self {
            Self::Parts(parts) => match parent.syntax {
                Syntax::InherentImpl { .. } => parts.contains(&child.id),
                Syntax::TraitImpl { .. } => false,
                Syntax::ItemFn { .. }
                | Syntax::ImplItemFn { .. }
                | Syntax::TraitItemFn { .. }
                | Syntax::ForeignItemFn { .. }
                | Syntax::ItemStruct { .. }
                | Syntax::Field { .. }
                | Syntax::ItemEnum { .. }
                | Syntax::Variant { .. }
                | Syntax::ItemTrait { .. } => true,
            },
            Self::Only(nodes) => nodes.contains(&child.id),
        }
    }
}

/// Whether `parent`'s declaration renders `child` inside it.
fn renders(parent: &Node, child: &Node) -> bool {
    matches!(
        (&parent.syntax, &child.syntax),
        (
            Syntax::ItemStruct { .. } | Syntax::Variant { .. },
            Syntax::Field { .. }
        ) | (Syntax::ItemEnum { .. }, Syntax::Variant { .. })
            | (Syntax::ItemTrait { .. }, Syntax::TraitItemFn { .. })
            | (
                Syntax::InherentImpl { .. } | Syntax::TraitImpl { .. },
                Syntax::ImplItemFn { .. }
            )
    )
}

/// Puts `node`'s justification first among `attrs`, as a doc comment, which the spec otherwise
/// leaves out.
fn justify(attrs: &mut Vec<Attribute>, node: &Node) {
    if let Some(justification) = &node.justification {
        let text = format!(" {}", justification.as_ref());
        attrs.insert(0, syn::parse_quote!(#[doc = #text]));
    }
}

/// A captured part of the surface. The capture's JSON maps it by an id of the capture's own and
/// lists it among its parent's children, so the JSON holds neither its id nor its parent.
#[derive(Serialize, Deserialize)]
pub(super) struct Node {
    #[serde(skip)]
    pub(super) id: NodeId,
    #[serde(skip)]
    pub(super) parent: Option<NodeId>,
    /// The node's justification, which the capture's JSON keeps beside its nodes.
    #[serde(skip)]
    pub(super) justification: Option<Justification>,
    mod_path: Vec<String>,
    kind: Kind,
    span: Span,
    /// Whether the node exists only in test builds, by its own attributes or those of an
    /// enclosing item or the file.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    test: bool,
    #[serde(flatten)]
    syntax: Syntax,
}

/// Nodes that render as one declaration.
enum Group<'a> {
    Node(&'a Node),
    /// Consecutive foreign fns of one `extern` block.
    Extern(&'a ForeignMod, Vec<&'a Node>),
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

    /// The block holding the stubs of `fns`, foreign fns which must all come from this block.
    fn declaration(&self, fns: Vec<&Node>) -> String {
        let items = fns
            .into_iter()
            .filter_map(|node| match &node.syntax {
                Syntax::ForeignItemFn { syntax, .. } => {
                    let mut syntax = syn::ForeignItemFn::from_adapter(syntax);
                    justify(&mut syntax.attrs, node);
                    Some(ForeignItem::Fn(syntax))
                }
                // A block holds only foreign fns.
                Syntax::ItemFn { .. }
                | Syntax::ImplItemFn { .. }
                | Syntax::TraitItemFn { .. }
                | Syntax::ItemStruct { .. }
                | Syntax::Field { .. }
                | Syntax::ItemEnum { .. }
                | Syntax::Variant { .. }
                | Syntax::ItemTrait { .. }
                | Syntax::InherentImpl { .. }
                | Syntax::TraitImpl { .. } => None,
            })
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
    Field,
    Variant,
}

/// A captured item's syntax after pruning, as syn-serde serializes it, beside the name of its syn
/// type, which tells apart syn types that serialize alike.
#[derive(Serialize, Deserialize)]
#[serde(tag = "syn")]
enum Syntax {
    ItemFn {
        syntax: <syn::ItemFn as Syn>::Adapter,
    },
    ImplItemFn {
        syntax: <syn::ImplItemFn as Syn>::Adapter,
    },
    TraitItemFn {
        syntax: <syn::TraitItemFn as Syn>::Adapter,
    },
    ForeignItemFn {
        foreign_mod: ForeignMod,
        syntax: <syn::ForeignItemFn as Syn>::Adapter,
    },
    /// A struct, without its fields, which are nodes of their own.
    ItemStruct {
        syntax: <syn::ItemStruct as Syn>::Adapter,
    },
    /// A field of a struct or of an enum's variant.
    Field {
        syntax: <syn::Field as Syn>::Adapter,
    },
    /// An enum, without its variants, which are nodes of their own.
    ItemEnum {
        syntax: <syn::ItemEnum as Syn>::Adapter,
    },
    /// An enum's variant, without its fields, which are nodes of their own.
    Variant {
        syntax: <syn::Variant as Syn>::Adapter,
    },
    ItemTrait {
        syntax: <syn::ItemTrait as Syn>::Adapter,
    },
    InherentImpl {
        syntax: <syn::ItemImpl as Syn>::Adapter,
    },
    TraitImpl {
        syntax: <syn::ItemImpl as Syn>::Adapter,
    },
}

impl Syntax {
    fn kind(&self) -> Kind {
        match self {
            Syntax::ItemFn { .. }
            | Syntax::ImplItemFn { .. }
            | Syntax::TraitItemFn { .. }
            | Syntax::ForeignItemFn { .. } => Kind::Fn,
            Syntax::ItemStruct { .. } => Kind::Struct,
            Syntax::ItemEnum { .. } => Kind::Enum,
            Syntax::ItemTrait { .. } => Kind::Trait,
            Syntax::InherentImpl { .. } | Syntax::TraitImpl { .. } => Kind::Impl,
            Syntax::Field { .. } => Kind::Field,
            Syntax::Variant { .. } => Kind::Variant,
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
fn trait_impl(mut syntax: syn::ItemImpl) -> String {
    syntax.items.clear();
    let text = unparse(Item::Impl(syntax));
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

#[derive(Default)]
struct Collector {
    nodes: Vec<Node>,
    /// Captured nodes enclosing the current syn node, innermost last.
    parents: Vec<NodeId>,
    mod_path: Vec<String>,
    /// The `extern` blocks walked so far.
    foreign_mods: usize,
    /// The first node surface-spec cannot capture.
    unmodeled: Option<Unmodeled>,
    /// Whether the current node exists only in test builds.
    test: bool,
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

    /// Walks `descend` for the node at `span`, which surface-spec cannot capture when syn-serde
    /// panics converting it, as it does on syntax its adapters do not model.
    fn guarded(&mut self, slot: Slot, span: proc_macro2::Span, descend: impl FnOnce(&mut Self)) {
        if panic::catch_unwind(AssertUnwindSafe(|| descend(self))).is_err() {
            self.unmodeled(slot, span);
        }
    }

    fn capture(
        &mut self,
        span: proc_macro2::Span,
        syntax: Syntax,
        descend: impl FnOnce(&mut Self),
    ) {
        let id = NodeId(self.nodes.len());
        self.nodes.push(Node {
            id,
            parent: self.parents.last().copied(),
            justification: None,
            mod_path: self.mod_path.clone(),
            kind: syntax.kind(),
            span: span.into(),
            test: self.test,
            syntax,
        });
        self.parents.push(id);
        descend(self);
        self.parents.pop();
    }

    fn capture_impl_item_fn(&mut self, span: proc_macro2::Span, node: &syn::ImplItemFn) {
        let pruned = Syntax::ImplItemFn {
            syntax: pruned(node, Prune::visit_impl_item_fn_mut),
        };
        self.capture(span, pruned, |collector| {
            visit::visit_impl_item_fn(collector, node)
        });
    }

    /// Walks `descend`, within which nodes exist only in test builds when `test` or the current
    /// node does.
    fn within(&mut self, test: bool, descend: impl FnOnce(&mut Self)) {
        let enclosing = self.test;
        self.test |= test;
        descend(self);
        self.test = enclosing;
    }

    fn capture_foreign_fn(&mut self, foreign_mod: ForeignMod, node: &syn::ForeignItemFn) {
        // The capture's signature keeps only `unsafe`, so `safe` would vanish from the spec.
        if let syn::Safety::Safe(_) = node.sig.safety {
            self.unmodeled(Slot::ForeignItem, node.span());
        }
        let pruned = Syntax::ForeignItemFn {
            foreign_mod,
            syntax: pruned(node, Prune::visit_foreign_item_fn_mut),
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
        self.within(test_item(node), |collector| match node {
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
            | Item::Use(_) => collector.guarded(Slot::Item, node.span(), |collector| {
                visit::visit_item(collector, node)
            }),
            _ => collector.unmodeled(Slot::Item, node.span()),
        });
    }

    fn visit_impl_item(&mut self, node: &'ast ImplItem) {
        self.within(test_impl_item(node), |collector| match node {
            ImplItem::Const(_) | ImplItem::Fn(_) | ImplItem::Type(_) | ImplItem::Macro(_) => {
                collector.guarded(Slot::ImplItem, node.span(), |collector| {
                    visit::visit_impl_item(collector, node)
                })
            }
            ImplItem::Verbatim(tokens) => match bodiless_fn(tokens) {
                Some(method) => collector.guarded(Slot::ImplItem, node.span(), |collector| {
                    collector.capture_impl_item_fn(node.span(), &method)
                }),
                None => collector.unmodeled(Slot::ImplItem, node.span()),
            },
            _ => collector.unmodeled(Slot::ImplItem, node.span()),
        });
    }

    fn visit_trait_item(&mut self, node: &'ast TraitItem) {
        self.within(test_trait_item(node), |collector| match node {
            TraitItem::Const(_) | TraitItem::Fn(_) | TraitItem::Type(_) | TraitItem::Macro(_) => {
                collector.guarded(Slot::TraitItem, node.span(), |collector| {
                    visit::visit_trait_item(collector, node)
                })
            }
            _ => collector.unmodeled(Slot::TraitItem, node.span()),
        });
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
                ForeignItem::Fn(fn_item) => self.within(test_only(&fn_item.attrs), |collector| {
                    collector.guarded(Slot::ForeignItem, item.span(), |collector| {
                        collector.capture_foreign_fn(ForeignMod::new(id, node), fn_item)
                    })
                }),
                ForeignItem::Static(_) | ForeignItem::Type(_) | ForeignItem::Macro(_) => self
                    .guarded(Slot::ForeignItem, item.span(), |collector| {
                        visit::visit_foreign_item(collector, item)
                    }),
                _ => self.unmodeled(Slot::ForeignItem, item.span()),
            }
        }
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let pruned = Syntax::ItemFn {
            syntax: pruned(node, Prune::visit_item_fn_mut),
        };
        self.capture(node.span(), pruned, |collector| {
            visit::visit_item_fn(collector, node)
        });
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        self.capture_impl_item_fn(node.span(), node);
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        let pruned = Syntax::TraitItemFn {
            syntax: pruned(node, Prune::visit_trait_item_fn_mut),
        };
        self.capture(node.span(), pruned, |collector| {
            visit::visit_trait_item_fn(collector, node)
        });
    }

    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        let pruned = Syntax::ItemStruct {
            syntax: pruned(node, Prune::visit_item_struct_mut),
        };
        self.capture(node.span(), pruned, |collector| {
            visit::visit_item_struct(collector, node)
        });
    }

    fn visit_item_enum(&mut self, node: &'ast syn::ItemEnum) {
        let pruned = Syntax::ItemEnum {
            syntax: pruned(node, Prune::visit_item_enum_mut),
        };
        self.capture(node.span(), pruned, |collector| {
            visit::visit_item_enum(collector, node)
        });
    }

    fn visit_field(&mut self, node: &'ast syn::Field) {
        let pruned = Syntax::Field {
            syntax: pruned(node, Prune::visit_field_mut),
        };
        self.within(test_only(&node.attrs), |collector| {
            collector.capture(node.span(), pruned, |collector| {
                visit::visit_field(collector, node)
            })
        });
    }

    fn visit_variant(&mut self, node: &'ast syn::Variant) {
        let pruned = Syntax::Variant {
            syntax: pruned(node, Prune::visit_variant_mut),
        };
        self.within(test_only(&node.attrs), |collector| {
            collector.capture(node.span(), pruned, |collector| {
                visit::visit_variant(collector, node)
            })
        });
    }

    // A union is no captured kind, so neither are its fields, though items inside them are.
    fn visit_item_union(&mut self, node: &'ast syn::ItemUnion) {
        self.visit_generics(&node.generics);
        for field in &node.fields.named {
            visit::visit_field(self, field);
        }
    }

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        let pruned = Syntax::ItemTrait {
            syntax: pruned(node, Prune::visit_item_trait_mut),
        };
        self.capture(node.span(), pruned, |collector| {
            visit::visit_item_trait(collector, node)
        });
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        let pruned = pruned(node, Prune::visit_item_impl_mut);
        let pruned = if node.trait_.is_some() {
            Syntax::TraitImpl { syntax: pruned }
        } else {
            Syntax::InherentImpl { syntax: pruned }
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

/// Keeps syn-serde's panics on syntax it does not model off stderr, since `Collector::guarded`
/// reports them as nodes surface-spec cannot capture. Other panics still reach the previous hook.
fn silence_syn_serde_panics() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            let in_syn_serde = info
                .location()
                .is_some_and(|location| location.file().contains("syn-serde3"));
            if !in_syn_serde {
                previous(info);
            }
        }));
    });
}

/// A clone of `node` with `prune` applied, as syn-serde serializes it.
fn pruned<N: Clone + Syn>(node: &N, prune: fn(&mut Prune, &mut N)) -> N::Adapter {
    let mut node = node.clone();
    prune(&mut Prune, &mut node);
    node.to_adapter()
}

/// Strips a captured node down to its own definition: no doc comments or inner attributes, empty
/// function bodies, and no captured kinds nested inside it, such as a struct's fields or an enum's
/// variants, since those are nodes of their own.
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
        visit_item_extern_crate_mut: ItemExternCrate,
        visit_item_macro_mut: ItemMacro,
        visit_item_static_mut: ItemStatic,
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
    }

    fn visit_lit_mut(&mut self, node: &mut syn::Lit) {
        // syn-serde serializes a byte literal as `52u8` but deserializes it expecting `b'4'`, so
        // it keeps the literal's own tokens instead.
        if let syn::Lit::Byte(byte) = node {
            *node = syn::Lit::Verbatim(byte.token());
        }
    }

    fn visit_item_struct_mut(&mut self, node: &mut syn::ItemStruct) {
        strip_attrs(&mut node.attrs);
        clear_fields(&mut node.fields);
        visit_mut::visit_item_struct_mut(self, node);
    }

    fn visit_item_enum_mut(&mut self, node: &mut syn::ItemEnum) {
        strip_attrs(&mut node.attrs);
        node.variants.clear();
        visit_mut::visit_item_enum_mut(self, node);
    }

    fn visit_variant_mut(&mut self, node: &mut syn::Variant) {
        strip_attrs(&mut node.attrs);
        clear_fields(&mut node.fields);
        visit_mut::visit_variant_mut(self, node);
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

fn clear_fields(fields: &mut syn::Fields) {
    match fields {
        syn::Fields::Named(fields) => fields.named.clear(),
        syn::Fields::Unnamed(fields) => fields.unnamed.clear(),
        syn::Fields::Unit => {}
    }
}

fn captured(item: &Item) -> bool {
    matches!(
        item,
        Item::Fn(_) | Item::Struct(_) | Item::Enum(_) | Item::Trait(_) | Item::Impl(_)
    )
}

/// Whether `item` exists only in test builds.
fn test_item(item: &Item) -> bool {
    test_only(match item {
        Item::Const(node) => &node.attrs,
        Item::Enum(node) => &node.attrs,
        Item::ExternCrate(node) => &node.attrs,
        Item::Fn(node) => &node.attrs,
        Item::ForeignMod(node) => &node.attrs,
        Item::Impl(node) => &node.attrs,
        Item::Macro(node) => &node.attrs,
        Item::Mod(node) => &node.attrs,
        Item::Static(node) => &node.attrs,
        Item::Struct(node) => &node.attrs,
        Item::Trait(node) => &node.attrs,
        Item::TraitAlias(node) => &node.attrs,
        Item::Type(node) => &node.attrs,
        Item::Union(node) => &node.attrs,
        Item::Use(node) => &node.attrs,
        _ => &[],
    })
}

/// Whether `item` exists only in test builds.
fn test_impl_item(item: &ImplItem) -> bool {
    match item {
        ImplItem::Const(node) => test_only(&node.attrs),
        ImplItem::Fn(node) => test_only(&node.attrs),
        ImplItem::Type(node) => test_only(&node.attrs),
        ImplItem::Macro(node) => test_only(&node.attrs),
        ImplItem::Verbatim(tokens) => {
            bodiless_fn(tokens).is_some_and(|node| test_only(&node.attrs))
        }
        _ => false,
    }
}

/// Whether `item` exists only in test builds.
fn test_trait_item(item: &TraitItem) -> bool {
    test_only(match item {
        TraitItem::Const(node) => &node.attrs,
        TraitItem::Fn(node) => &node.attrs,
        TraitItem::Type(node) => &node.attrs,
        TraitItem::Macro(node) => &node.attrs,
        _ => &[],
    })
}

/// Whether `attrs` keep their item out of non-test builds: a test attribute such as `#[test]` or
/// `#[tokio::test]`, or a `#[cfg(...)]` whose predicate holds only when `test` is set.
fn test_only(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| match &attr.meta {
        Meta::List(list) if list.path.is_ident("cfg") => list
            .parse_args::<Meta>()
            .is_ok_and(|predicate| requires_test(&predicate)),
        meta => meta
            .path()
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "test"),
    })
}

/// Whether a cfg predicate is false whenever `test` is unset.
fn requires_test(predicate: &Meta) -> bool {
    match predicate {
        Meta::Path(path) => path.is_ident("test"),
        Meta::List(list) => {
            let Ok(predicates) =
                list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
            else {
                return false;
            };
            if list.path.is_ident("all") {
                predicates.iter().any(requires_test)
            } else if list.path.is_ident("any") {
                !predicates.is_empty() && predicates.iter().all(requires_test)
            } else {
                false
            }
        }
        Meta::NameValue(_) => false,
    }
}

/// Whether `vis` limits an item to its own module: no visibility, `pub(self)`, or `pub(in self)`.
fn private(vis: &Visibility) -> bool {
    match vis {
        Visibility::Inherited => true,
        Visibility::Restricted(restricted) => restricted.path.is_ident("self"),
        Visibility::Public(_) => false,
    }
}
