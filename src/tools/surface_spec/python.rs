//! Python surfaces: a model of the captured defs and classes holding syntax facts only, printed
//! back as Python and laid out by the Ruff formatter.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use ruff_python_ast as ast;
use ruff_python_ast::statement_visitor::{StatementVisitor, walk_stmt};
use ruff_python_ast::{Stmt, StringFlags};
use ruff_python_formatter::{PyFormatOptions, format_module_source};
use ruff_python_parser::parse_module;
use ruff_source_file::{LineColumn, LineIndex};
use ruff_text_size::{TextRange, TextSize};
use serde::{Deserialize, Serialize};

use super::{Part, Position, RecordId, Span};

#[derive(Serialize, Deserialize)]
pub(super) struct FileSurface {
    path: PathBuf,
    records: Vec<Record>,
}

impl FileSurface {
    /// The surface of `source`, which fails on any syntax error.
    pub(super) fn parse(path: PathBuf, source: &str) -> Result<Self> {
        let source = Source::new(source);
        let parsed = match parse_module(source.text) {
            Ok(parsed) => parsed,
            Err(error) => {
                let start = source.position(error.location.start());
                bail!(
                    "parsing {}:{}:{}: {}",
                    path.display(),
                    start.line,
                    start.column + 1,
                    error.error,
                );
            }
        };
        let mut collector = Collector {
            source,
            records: Vec::new(),
            parents: Vec::new(),
            unmodeled: None,
        };
        collector.visit_body(&parsed.syntax().body);
        if let Some(Unmodeled(range)) = collector.unmodeled {
            let start = collector.source.position(range.start());
            bail!(
                "parsing {}:{}:{}: the surface model has no form for this IPython escape command",
                path.display(),
                start.line,
                start.column + 1,
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

    /// The declarations a change adds or deletes as a whole: every def, every class attribute,
    /// and every class with neither attributes nor records inside it. A class holding either is
    /// no part of its own, since what it holds is.
    pub(super) fn parts(&self) -> Vec<Part> {
        let mut parts = Vec::new();
        for record in &self.records {
            match &record.node {
                Node::Def(_) => parts.push(Part::Record(record.id)),
                Node::Class(class) => {
                    parts.extend(
                        (0..class.attributes.len()).map(|index| Part::Attribute(record.id, index)),
                    );
                    if class.attributes.is_empty() && self.children(record.id).next().is_none() {
                        parts.push(Part::Record(record.id));
                    }
                }
            }
        }
        parts
    }

    /// The declarations of `parts` as Python text in the spec's stub style, in record order,
    /// laid out by the Ruff formatter.
    pub(super) fn declarations(&self, parts: &BTreeSet<Part>) -> Result<String> {
        let formatted = format_module_source(&self.text(parts), PyFormatOptions::default())
            .with_context(|| format!("formatting the declarations of {}", self.path.display()))?;
        Ok(formatted.into_code().trim_end().to_owned())
    }

    pub(super) fn text(&self, parts: &BTreeSet<Part>) -> String {
        let selected = Selected {
            parts,
            records: self.enclosing(parts),
        };
        let mut printer = Printer::default();
        let top_level = self
            .records
            .iter()
            .filter(|record| record.parent.is_none() && selected.records.contains(&record.id));
        self.block(&mut printer, top_level, &selected);
        printer.text
    }

    fn enclosing(&self, parts: &BTreeSet<Part>) -> BTreeSet<RecordId> {
        let mut records = BTreeSet::new();
        for part in parts {
            let mut next = Some(part.record());
            while let Some(id) = next
                && records.insert(id)
            {
                next = self.records[id.0].parent;
            }
        }
        records
    }

    /// Prints `record` with the selected records it encloses inside it: a function's body
    /// holds them, or is `...` without them, and a class's body holds its selected attributes
    /// and then them.
    fn declaration(&self, printer: &mut Printer, record: &Record, selected: &Selected) {
        let mut children = self
            .children(record.id)
            .filter(|child| selected.records.contains(&child.id))
            .peekable();
        match &record.node {
            Node::Def(function) => {
                printer.function_header(function);
                if children.peek().is_none() {
                    printer.push(" ...\n");
                    return;
                }
                printer.push("\n");
                printer.indent += 1;
                self.block(printer, children, selected);
                printer.indent -= 1;
            }
            Node::Class(class) => {
                let attributes: Vec<&Attribute> = class
                    .attributes
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| {
                        selected.parts.contains(&Part::Attribute(record.id, *index))
                    })
                    .map(|(_, attribute)| attribute)
                    .collect();
                printer.class_header(class);
                printer.push("\n");
                printer.indent += 1;
                for attribute in &attributes {
                    printer.attribute(attribute);
                }
                if attributes.is_empty() && children.peek().is_none() {
                    printer.indentation();
                    printer.push("...\n");
                } else if !attributes.is_empty() && children.peek().is_some() {
                    printer.push("\n");
                }
                self.block(printer, children, selected);
                printer.indent -= 1;
            }
        }
    }

    fn block<'a>(
        &'a self,
        printer: &mut Printer,
        records: impl Iterator<Item = &'a Record>,
        selected: &Selected,
    ) {
        for (index, record) in records.enumerate() {
            if index > 0 {
                // The formatter keeps consecutive stubs together as overloads when the source has
                // no blank line between them.
                printer.push("\n");
            }
            self.declaration(printer, record, selected);
        }
    }

    fn children(&self, id: RecordId) -> impl Iterator<Item = &Record> {
        self.records
            .iter()
            .filter(move |record| record.parent == Some(id))
    }
}

struct Selected<'a> {
    parts: &'a BTreeSet<Part>,
    records: BTreeSet<RecordId>,
}

#[derive(Serialize, Deserialize)]
struct Record {
    id: RecordId,
    parent: Option<RecordId>,
    span: Span,
    #[serde(flatten)]
    node: Node,
}

/// A captured statement beside its kind.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "node", rename_all = "lowercase")]
enum Node {
    Def(Box<Function>),
    Class(Class),
}

/// A `def` without its body.
#[derive(Serialize, Deserialize)]
struct Function {
    name: String,
    is_async: bool,
    decorators: Vec<Expr>,
    type_params: Vec<TypeParam>,
    parameters: Parameters,
    returns: Option<Expr>,
}

/// A `class` with the attributes its body declares directly.
#[derive(Serialize, Deserialize)]
struct Class {
    name: String,
    decorators: Vec<Expr>,
    type_params: Vec<TypeParam>,
    bases: Vec<Expr>,
    keywords: Vec<Keyword>,
    attributes: Vec<Attribute>,
}

#[derive(Default, Serialize, Deserialize)]
struct Parameters {
    posonly: Vec<Parameter>,
    args: Vec<Parameter>,
    vararg: Option<Variadic>,
    kwonly: Vec<Parameter>,
    kwarg: Option<Variadic>,
}

impl Parameters {
    fn is_empty(&self) -> bool {
        self.posonly.is_empty()
            && self.args.is_empty()
            && self.vararg.is_none()
            && self.kwonly.is_empty()
            && self.kwarg.is_none()
    }
}

#[derive(Serialize, Deserialize)]
struct Parameter {
    name: String,
    annotation: Option<Expr>,
    default: Option<Expr>,
}

/// `*args` or `**kwargs`, which take no default.
#[derive(Serialize, Deserialize)]
struct Variadic {
    name: String,
    annotation: Option<Expr>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum TypeParam {
    TypeVar {
        name: String,
        bound: Option<Expr>,
        default: Option<Expr>,
    },
    ParamSpec {
        name: String,
        default: Option<Expr>,
    },
    TypeVarTuple {
        name: String,
        default: Option<Expr>,
    },
}

/// A keyword argument, or `**value` when `arg` is absent.
#[derive(Serialize, Deserialize)]
struct Keyword {
    arg: Option<String>,
    value: Expr,
}

/// A statement directly in a class body that declares an attribute.
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Attribute {
    AnnAssign {
        target: Expr,
        annotation: Expr,
        value: Option<Expr>,
    },
    Assign {
        targets: Vec<Expr>,
        value: Expr,
    },
    TypeAlias {
        name: Expr,
        type_params: Vec<TypeParam>,
        value: Expr,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Expr {
    BoolOp {
        op: BoolOp,
        values: Vec<Expr>,
    },
    Named {
        target: Box<Expr>,
        value: Box<Expr>,
    },
    BinOp {
        left: Box<Expr>,
        op: Operator,
        right: Box<Expr>,
    },
    UnaryOp {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Lambda {
        parameters: Box<Parameters>,
        body: Box<Expr>,
    },
    If {
        test: Box<Expr>,
        body: Box<Expr>,
        orelse: Box<Expr>,
    },
    Dict {
        items: Vec<DictItem>,
    },
    Set {
        elts: Vec<Expr>,
    },
    ListComp {
        elt: Box<Expr>,
        generators: Vec<Comprehension>,
    },
    SetComp {
        elt: Box<Expr>,
        generators: Vec<Comprehension>,
    },
    /// `{key: value for ...}`, or `{**value for ...}` when `key` is absent.
    DictComp {
        key: Option<Box<Expr>>,
        value: Box<Expr>,
        generators: Vec<Comprehension>,
    },
    Generator {
        elt: Box<Expr>,
        generators: Vec<Comprehension>,
    },
    Await {
        value: Box<Expr>,
    },
    Yield {
        value: Option<Box<Expr>>,
    },
    YieldFrom {
        value: Box<Expr>,
    },
    /// `left op right ...`, a chain of one or more comparisons.
    Compare {
        left: Box<Expr>,
        comparisons: Vec<Comparison>,
    },
    Call {
        func: Box<Expr>,
        args: Vec<Expr>,
        keywords: Vec<Keyword>,
    },
    /// An f-string's implicitly concatenated parts, as Ruff gives them.
    FString {
        parts: Vec<FStringPart>,
    },
    /// A t-string's implicitly concatenated parts, as Ruff gives them.
    TString {
        parts: Vec<TString>,
    },
    /// A string's implicitly concatenated parts, as Ruff gives them.
    StringLiteral {
        parts: Vec<StringPart>,
    },
    /// A bytes literal's implicitly concatenated parts, as Ruff gives them.
    BytesLiteral {
        parts: Vec<BytesPart>,
    },
    NumberLiteral {
        value: Number,
    },
    BooleanLiteral {
        value: bool,
    },
    NoneLiteral,
    EllipsisLiteral,
    Attribute {
        value: Box<Expr>,
        attr: String,
    },
    Subscript {
        value: Box<Expr>,
        slice: Box<Expr>,
    },
    Starred {
        value: Box<Expr>,
    },
    Name {
        id: String,
    },
    List {
        elts: Vec<Expr>,
    },
    Tuple {
        elts: Vec<Expr>,
    },
    Slice {
        lower: Option<Box<Expr>>,
        upper: Option<Box<Expr>>,
        step: Option<Box<Expr>>,
    },
}

/// A dict display entry, or `**value` when `key` is absent.
#[derive(Serialize, Deserialize)]
struct DictItem {
    key: Option<Expr>,
    value: Expr,
}

#[derive(Serialize, Deserialize)]
struct Comprehension {
    target: Expr,
    iter: Expr,
    ifs: Vec<Expr>,
    is_async: bool,
}

#[derive(Serialize, Deserialize)]
struct Comparison {
    op: CmpOp,
    right: Expr,
}

/// A string part: Ruff's decoded value and its flags.
#[derive(Serialize, Deserialize)]
struct StringPart {
    value: String,
    flags: Flags<StringPrefix>,
}

/// A bytes part: Ruff's decoded value and its flags.
#[derive(Serialize, Deserialize)]
struct BytesPart {
    value: Vec<u8>,
    flags: Flags<BytesPrefix>,
}

/// A part of an f-string, which Ruff gives as a plain string or an f-string.
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum FStringPart {
    Literal(StringPart),
    FString(FString),
}

#[derive(Serialize, Deserialize)]
struct FString {
    elements: Vec<Element>,
    flags: Flags<FStringPrefix>,
}

#[derive(Serialize, Deserialize)]
struct TString {
    elements: Vec<Element>,
    flags: Flags<TStringPrefix>,
}

/// A string part's flags as Ruff reports them: its prefix, quote style, and triple quoting.
#[derive(Serialize, Deserialize)]
struct Flags<P> {
    prefix: P,
    quote_style: Quote,
    triple_quoted: bool,
}

impl<P: Prefix> Flags<P> {
    fn new(prefix: P, flags: impl StringFlags) -> Self {
        Self {
            prefix,
            quote_style: flags.quote_style().into(),
            triple_quoted: flags.is_triple_quoted(),
        }
    }

    fn quote(&self) -> char {
        match self.quote_style {
            Quote::Single => '\'',
            Quote::Double => '"',
        }
    }

    fn is_raw(&self) -> bool {
        self.prefix.is_raw()
    }
}

/// A string prefix as source text.
trait Prefix: Copy {
    fn text(self) -> &'static str;
    fn is_raw(self) -> bool;
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Quote {
    Single,
    Double,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StringPrefix {
    Empty,
    Unicode,
    Raw { uppercase: bool },
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum BytesPrefix {
    Regular,
    Raw { uppercase_r: bool },
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum FStringPrefix {
    Regular,
    Raw { uppercase_r: bool },
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum TStringPrefix {
    Regular,
    Raw { uppercase_r: bool },
}

impl Prefix for StringPrefix {
    fn text(self) -> &'static str {
        match self {
            Self::Empty => "",
            Self::Unicode => "u",
            Self::Raw { uppercase: true } => "R",
            Self::Raw { uppercase: false } => "r",
        }
    }

    fn is_raw(self) -> bool {
        match self {
            Self::Empty | Self::Unicode => false,
            Self::Raw { .. } => true,
        }
    }
}

impl Prefix for BytesPrefix {
    fn text(self) -> &'static str {
        match self {
            Self::Regular => "b",
            Self::Raw { uppercase_r: true } => "Rb",
            Self::Raw { uppercase_r: false } => "rb",
        }
    }

    fn is_raw(self) -> bool {
        match self {
            Self::Regular => false,
            Self::Raw { .. } => true,
        }
    }
}

impl Prefix for FStringPrefix {
    fn text(self) -> &'static str {
        match self {
            Self::Regular => "f",
            Self::Raw { uppercase_r: true } => "Rf",
            Self::Raw { uppercase_r: false } => "rf",
        }
    }

    fn is_raw(self) -> bool {
        match self {
            Self::Regular => false,
            Self::Raw { .. } => true,
        }
    }
}

impl Prefix for TStringPrefix {
    fn text(self) -> &'static str {
        match self {
            Self::Regular => "t",
            Self::Raw { uppercase_r: true } => "Rt",
            Self::Raw { uppercase_r: false } => "rt",
        }
    }

    fn is_raw(self) -> bool {
        match self {
            Self::Regular => false,
            Self::Raw { .. } => true,
        }
    }
}

/// A piece of an f-string or t-string.
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Element {
    /// Ruff's decoded text.
    Literal { value: String },
    Interpolation {
        expression: Expr,
        debug_text: Option<DebugText>,
        conversion: Conversion,
        format_spec: Option<Vec<Element>>,
    },
}

/// The text of a self-documenting `{expression=}` as Ruff splits it.
#[derive(Serialize, Deserialize)]
struct DebugText {
    /// The text between `{` and the expression.
    leading: String,
    /// The expression's source text.
    expression: String,
    /// The text between the expression and the conversion, format spec, or `}`.
    trailing: String,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Conversion {
    None,
    Str,
    Ascii,
    Repr,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Number {
    /// Ruff's text for an int: decimal when it fits a `u64`, else the literal as written.
    Int {
        value: String,
    },
    Float {
        value: f64,
    },
    Complex {
        real: f64,
        imag: f64,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum BoolOp {
    And,
    Or,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Operator {
    Add,
    Sub,
    Mult,
    MatMult,
    Div,
    Mod,
    Pow,
    LShift,
    RShift,
    BitOr,
    BitXor,
    BitAnd,
    FloorDiv,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum UnaryOp {
    Invert,
    Not,
    UAdd,
    USub,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CmpOp {
    Eq,
    NotEq,
    Lt,
    LtE,
    Gt,
    GtE,
    Is,
    IsNot,
    In,
    NotIn,
}

/// Syntax the model has no form for: an IPython escape command, which Ruff parses only outside
/// module mode.
struct Unmodeled(TextRange);

/// One source's text and line index, against which captured nodes convert to the model.
struct Source<'src> {
    text: &'src str,
    index: LineIndex,
}

impl From<LineColumn> for Position {
    fn from(location: LineColumn) -> Self {
        Self {
            line: location.line.get(),
            column: location.column.to_zero_indexed(),
        }
    }
}

/// Walks every statement of a module, recording each def and class.
struct Collector<'src> {
    source: Source<'src>,
    records: Vec<Record>,
    /// Captured records enclosing the current statement, innermost last.
    parents: Vec<RecordId>,
    /// The first syntax the model has no form for.
    unmodeled: Option<Unmodeled>,
}

impl Collector<'_> {
    fn unmodeled(&mut self, unmodeled: Unmodeled) {
        self.unmodeled.get_or_insert(unmodeled);
    }

    fn capture(
        &mut self,
        range: TextRange,
        node: Result<Node, Unmodeled>,
        descend: impl FnOnce(&mut Self),
    ) {
        let node = match node {
            Ok(node) => node,
            Err(unmodeled) => {
                self.unmodeled(unmodeled);
                return;
            }
        };
        let id = RecordId(self.records.len());
        self.records.push(Record {
            id,
            parent: self.parents.last().copied(),
            span: self.source.span(range),
            node,
        });
        self.parents.push(id);
        descend(self);
        self.parents.pop();
    }
}

// Each statement kind is listed, so any kind added after this code fails to compile instead of
// passing unwalked.
impl<'a> StatementVisitor<'a> for Collector<'_> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::FunctionDef(node) => {
                let function = self
                    .source
                    .function(node)
                    .map(|function| Node::Def(Box::new(function)));
                self.capture(node.range, function, |collector| walk_stmt(collector, stmt));
            }
            Stmt::ClassDef(node) => {
                let class = self.source.class(node).map(Node::Class);
                self.capture(node.range, class, |collector| walk_stmt(collector, stmt));
            }
            Stmt::IpyEscapeCommand(node) => self.unmodeled(Unmodeled(node.range)),
            Stmt::Return(_)
            | Stmt::Delete(_)
            | Stmt::TypeAlias(_)
            | Stmt::Assign(_)
            | Stmt::AugAssign(_)
            | Stmt::AnnAssign(_)
            | Stmt::For(_)
            | Stmt::While(_)
            | Stmt::If(_)
            | Stmt::With(_)
            | Stmt::Match(_)
            | Stmt::Raise(_)
            | Stmt::Try(_)
            | Stmt::Assert(_)
            | Stmt::Import(_)
            | Stmt::ImportFrom(_)
            | Stmt::Global(_)
            | Stmt::Nonlocal(_)
            | Stmt::Expr(_)
            | Stmt::Pass(_)
            | Stmt::Break(_)
            | Stmt::Continue(_) => walk_stmt(self, stmt),
        }
    }
}

impl<'src> Source<'src> {
    fn new(text: &'src str) -> Self {
        Self {
            text,
            index: LineIndex::from_source_text(text),
        }
    }

    fn position(&self, offset: TextSize) -> Position {
        self.index.line_column(offset, self.text).into()
    }

    fn span(&self, range: TextRange) -> Span {
        Span {
            start: self.position(range.start()),
            end: self.position(range.end()),
        }
    }

    fn function(&self, node: &ast::StmtFunctionDef) -> Result<Function, Unmodeled> {
        Ok(Function {
            name: node.name.as_str().to_owned(),
            is_async: node.is_async,
            decorators: self.decorators(&node.decorator_list)?,
            type_params: self.type_params(node.type_params.as_deref())?,
            parameters: self.parameters(&node.parameters)?,
            returns: self.optional(node.returns.as_deref())?,
        })
    }

    fn class(&self, node: &ast::StmtClassDef) -> Result<Class, Unmodeled> {
        let (bases, keywords) = match node.arguments.as_deref() {
            Some(arguments) => (
                self.exprs(&arguments.args)?,
                self.keywords(&arguments.keywords)?,
            ),
            None => (Vec::new(), Vec::new()),
        };
        Ok(Class {
            name: node.name.as_str().to_owned(),
            decorators: self.decorators(&node.decorator_list)?,
            type_params: self.type_params(node.type_params.as_deref())?,
            bases,
            keywords,
            attributes: node
                .body
                .iter()
                .filter_map(|stmt| self.attribute(stmt).transpose())
                .collect::<Result<_, _>>()?,
        })
    }

    /// The attribute `stmt` declares directly in a class body, if it declares one.
    fn attribute(&self, stmt: &Stmt) -> Result<Option<Attribute>, Unmodeled> {
        Ok(match stmt {
            Stmt::AnnAssign(node) => Some(Attribute::AnnAssign {
                target: self.expr(&node.target)?,
                annotation: self.expr(&node.annotation)?,
                value: self.optional(node.value.as_deref())?,
            }),
            Stmt::Assign(node) => Some(Attribute::Assign {
                targets: self.exprs(&node.targets)?,
                value: self.expr(&node.value)?,
            }),
            Stmt::TypeAlias(node) => Some(Attribute::TypeAlias {
                name: self.expr(&node.name)?,
                type_params: self.type_params(node.type_params.as_deref())?,
                value: self.expr(&node.value)?,
            }),
            Stmt::IpyEscapeCommand(node) => return Err(Unmodeled(node.range)),
            Stmt::FunctionDef(_)
            | Stmt::ClassDef(_)
            | Stmt::Return(_)
            | Stmt::Delete(_)
            | Stmt::AugAssign(_)
            | Stmt::For(_)
            | Stmt::While(_)
            | Stmt::If(_)
            | Stmt::With(_)
            | Stmt::Match(_)
            | Stmt::Raise(_)
            | Stmt::Try(_)
            | Stmt::Assert(_)
            | Stmt::Import(_)
            | Stmt::ImportFrom(_)
            | Stmt::Global(_)
            | Stmt::Nonlocal(_)
            | Stmt::Expr(_)
            | Stmt::Pass(_)
            | Stmt::Break(_)
            | Stmt::Continue(_) => None,
        })
    }

    fn parameters(&self, node: &ast::Parameters) -> Result<Parameters, Unmodeled> {
        Ok(Parameters {
            posonly: self.parameter_list(&node.posonlyargs)?,
            args: self.parameter_list(&node.args)?,
            vararg: node
                .vararg
                .as_deref()
                .map(|vararg| self.variadic(vararg))
                .transpose()?,
            kwonly: self.parameter_list(&node.kwonlyargs)?,
            kwarg: node
                .kwarg
                .as_deref()
                .map(|kwarg| self.variadic(kwarg))
                .transpose()?,
        })
    }

    fn parameter_list(
        &self,
        nodes: &[ast::ParameterWithDefault],
    ) -> Result<Vec<Parameter>, Unmodeled> {
        nodes
            .iter()
            .map(|node| {
                Ok(Parameter {
                    name: node.parameter.name.as_str().to_owned(),
                    annotation: self.optional(node.parameter.annotation.as_deref())?,
                    default: self.optional(node.default.as_deref())?,
                })
            })
            .collect()
    }

    fn variadic(&self, node: &ast::Parameter) -> Result<Variadic, Unmodeled> {
        Ok(Variadic {
            name: node.name.as_str().to_owned(),
            annotation: self.optional(node.annotation.as_deref())?,
        })
    }

    fn type_params(&self, node: Option<&ast::TypeParams>) -> Result<Vec<TypeParam>, Unmodeled> {
        node.into_iter()
            .flat_map(|params| params.iter())
            .map(|param| {
                Ok(match param {
                    ast::TypeParam::TypeVar(node) => TypeParam::TypeVar {
                        name: node.name.as_str().to_owned(),
                        bound: self.optional(node.bound.as_deref())?,
                        default: self.optional(node.default.as_deref())?,
                    },
                    ast::TypeParam::ParamSpec(node) => TypeParam::ParamSpec {
                        name: node.name.as_str().to_owned(),
                        default: self.optional(node.default.as_deref())?,
                    },
                    ast::TypeParam::TypeVarTuple(node) => TypeParam::TypeVarTuple {
                        name: node.name.as_str().to_owned(),
                        default: self.optional(node.default.as_deref())?,
                    },
                })
            })
            .collect()
    }

    fn keywords(&self, nodes: &[ast::Keyword]) -> Result<Vec<Keyword>, Unmodeled> {
        nodes
            .iter()
            .map(|node| {
                Ok(Keyword {
                    arg: node.arg.as_ref().map(|arg| arg.as_str().to_owned()),
                    value: self.expr(&node.value)?,
                })
            })
            .collect()
    }

    fn comprehensions(
        &self,
        nodes: &[ast::Comprehension],
    ) -> Result<Vec<Comprehension>, Unmodeled> {
        nodes
            .iter()
            .map(|node| {
                Ok(Comprehension {
                    target: self.expr(&node.target)?,
                    iter: self.expr(&node.iter)?,
                    ifs: self.exprs(&node.ifs)?,
                    is_async: node.is_async,
                })
            })
            .collect()
    }

    fn decorators(&self, nodes: &[ast::Decorator]) -> Result<Vec<Expr>, Unmodeled> {
        nodes
            .iter()
            .map(|decorator| self.expr(&decorator.expression))
            .collect()
    }

    fn exprs(&self, nodes: &[ast::Expr]) -> Result<Vec<Expr>, Unmodeled> {
        nodes.iter().map(|node| self.expr(node)).collect()
    }

    fn optional(&self, node: Option<&ast::Expr>) -> Result<Option<Expr>, Unmodeled> {
        node.map(|node| self.expr(node)).transpose()
    }

    fn boxed(&self, node: &ast::Expr) -> Result<Box<Expr>, Unmodeled> {
        self.expr(node).map(Box::new)
    }

    fn optional_boxed(&self, node: Option<&ast::Expr>) -> Result<Option<Box<Expr>>, Unmodeled> {
        Ok(self.optional(node)?.map(Box::new))
    }

    // Each expression kind is listed, so any kind added after this code fails to compile
    // instead of passing unmodeled.
    fn expr(&self, node: &ast::Expr) -> Result<Expr, Unmodeled> {
        Ok(match node {
            ast::Expr::BoolOp(node) => Expr::BoolOp {
                op: node.op.into(),
                values: self.exprs(&node.values)?,
            },
            ast::Expr::Named(node) => Expr::Named {
                target: self.boxed(&node.target)?,
                value: self.boxed(&node.value)?,
            },
            ast::Expr::BinOp(node) => Expr::BinOp {
                left: self.boxed(&node.left)?,
                op: node.op.into(),
                right: self.boxed(&node.right)?,
            },
            ast::Expr::UnaryOp(node) => Expr::UnaryOp {
                op: node.op.into(),
                operand: self.boxed(&node.operand)?,
            },
            ast::Expr::Lambda(node) => Expr::Lambda {
                parameters: Box::new(match node.parameters.as_deref() {
                    Some(parameters) => self.parameters(parameters)?,
                    None => Parameters::default(),
                }),
                body: self.boxed(&node.body)?,
            },
            ast::Expr::If(node) => Expr::If {
                test: self.boxed(&node.test)?,
                body: self.boxed(&node.body)?,
                orelse: self.boxed(&node.orelse)?,
            },
            ast::Expr::Dict(node) => Expr::Dict {
                items: node
                    .items
                    .iter()
                    .map(|item| {
                        Ok(DictItem {
                            key: self.optional(item.key.as_ref())?,
                            value: self.expr(&item.value)?,
                        })
                    })
                    .collect::<Result<_, _>>()?,
            },
            ast::Expr::Set(node) => Expr::Set {
                elts: self.exprs(&node.elts)?,
            },
            ast::Expr::ListComp(node) => Expr::ListComp {
                elt: self.boxed(&node.elt)?,
                generators: self.comprehensions(&node.generators)?,
            },
            ast::Expr::SetComp(node) => Expr::SetComp {
                elt: self.boxed(&node.elt)?,
                generators: self.comprehensions(&node.generators)?,
            },
            ast::Expr::DictComp(node) => Expr::DictComp {
                key: self.optional_boxed(node.key.as_deref())?,
                value: self.boxed(&node.value)?,
                generators: self.comprehensions(&node.generators)?,
            },
            ast::Expr::Generator(node) => Expr::Generator {
                elt: self.boxed(&node.elt)?,
                generators: self.comprehensions(&node.generators)?,
            },
            ast::Expr::Await(node) => Expr::Await {
                value: self.boxed(&node.value)?,
            },
            ast::Expr::Yield(node) => Expr::Yield {
                value: self.optional_boxed(node.value.as_deref())?,
            },
            ast::Expr::YieldFrom(node) => Expr::YieldFrom {
                value: self.boxed(&node.value)?,
            },
            ast::Expr::Compare(node) => {
                let (left, rights) = node
                    .operands
                    .split_first()
                    .expect("Ruff gives a comparison its left operand");
                Expr::Compare {
                    left: self.boxed(left)?,
                    comparisons: node
                        .ops
                        .iter()
                        .zip(rights)
                        .map(|(op, right)| {
                            Ok(Comparison {
                                op: (*op).into(),
                                right: self.expr(right)?,
                            })
                        })
                        .collect::<Result<_, _>>()?,
                }
            }
            ast::Expr::Call(node) => Expr::Call {
                func: self.boxed(&node.func)?,
                args: self.exprs(&node.arguments.args)?,
                keywords: self.keywords(&node.arguments.keywords)?,
            },
            ast::Expr::FString(node) => Expr::FString {
                parts: node
                    .value
                    .iter()
                    .map(|part| {
                        Ok(match part {
                            ast::FStringPartRef::Literal(literal) => {
                                FStringPart::Literal(StringPart::from(literal))
                            }
                            ast::FStringPartRef::FString(fstring) => {
                                FStringPart::FString(FString {
                                    elements: self.elements(&fstring.elements)?,
                                    flags: Flags::new(fstring.flags.prefix().into(), fstring.flags),
                                })
                            }
                        })
                    })
                    .collect::<Result<_, _>>()?,
            },
            ast::Expr::TString(node) => Expr::TString {
                parts: node
                    .value
                    .iter()
                    .map(|tstring| {
                        Ok(TString {
                            elements: self.elements(&tstring.elements)?,
                            flags: Flags::new(tstring.flags.prefix().into(), tstring.flags),
                        })
                    })
                    .collect::<Result<_, _>>()?,
            },
            ast::Expr::StringLiteral(node) => Expr::StringLiteral {
                parts: node.value.iter().map(StringPart::from).collect(),
            },
            ast::Expr::BytesLiteral(node) => Expr::BytesLiteral {
                parts: node.value.iter().map(BytesPart::from).collect(),
            },
            ast::Expr::NumberLiteral(node) => Expr::NumberLiteral {
                value: Number::from(&node.value),
            },
            ast::Expr::BooleanLiteral(node) => Expr::BooleanLiteral { value: node.value },
            ast::Expr::NoneLiteral(_) => Expr::NoneLiteral,
            ast::Expr::EllipsisLiteral(_) => Expr::EllipsisLiteral,
            ast::Expr::Attribute(node) => Expr::Attribute {
                value: self.boxed(&node.value)?,
                attr: node.attr.as_str().to_owned(),
            },
            ast::Expr::Subscript(node) => Expr::Subscript {
                value: self.boxed(&node.value)?,
                slice: self.boxed(&node.slice)?,
            },
            ast::Expr::Starred(node) => Expr::Starred {
                value: self.boxed(&node.value)?,
            },
            ast::Expr::Name(node) => Expr::Name {
                id: node.id.as_str().to_owned(),
            },
            ast::Expr::List(node) => Expr::List {
                elts: self.exprs(&node.elts)?,
            },
            ast::Expr::Tuple(node) => Expr::Tuple {
                elts: self.exprs(&node.elts)?,
            },
            ast::Expr::Slice(node) => Expr::Slice {
                lower: self.optional_boxed(node.lower.as_deref())?,
                upper: self.optional_boxed(node.upper.as_deref())?,
                step: self.optional_boxed(node.step.as_deref())?,
            },
            ast::Expr::IpyEscapeCommand(node) => return Err(Unmodeled(node.range)),
        })
    }

    fn elements(
        &self,
        elements: &ast::InterpolatedStringElements,
    ) -> Result<Vec<Element>, Unmodeled> {
        elements
            .iter()
            .map(|element| {
                Ok(match element {
                    ast::InterpolatedStringElement::Literal(literal) => Element::Literal {
                        value: literal.value.to_string(),
                    },
                    ast::InterpolatedStringElement::Interpolation(node) => Element::Interpolation {
                        expression: self.expr(&node.expression)?,
                        debug_text: node.debug_text.as_ref().map(DebugText::from),
                        conversion: node.conversion.into(),
                        format_spec: node
                            .format_spec
                            .as_deref()
                            .map(|spec| self.elements(&spec.elements))
                            .transpose()?,
                    },
                })
            })
            .collect()
    }
}

impl From<&ast::StringLiteral> for StringPart {
    fn from(literal: &ast::StringLiteral) -> Self {
        Self {
            value: literal.value.to_string(),
            flags: Flags::new(literal.flags.prefix().into(), literal.flags),
        }
    }
}

impl From<&ast::BytesLiteral> for BytesPart {
    fn from(literal: &ast::BytesLiteral) -> Self {
        Self {
            value: literal.value.to_vec(),
            flags: Flags::new(literal.flags.prefix().into(), literal.flags),
        }
    }
}

impl From<ast::str::Quote> for Quote {
    fn from(quote: ast::str::Quote) -> Self {
        match quote {
            ast::str::Quote::Single => Self::Single,
            ast::str::Quote::Double => Self::Double,
        }
    }
}

impl From<ast::str_prefix::StringLiteralPrefix> for StringPrefix {
    fn from(prefix: ast::str_prefix::StringLiteralPrefix) -> Self {
        match prefix {
            ast::str_prefix::StringLiteralPrefix::Empty => Self::Empty,
            ast::str_prefix::StringLiteralPrefix::Unicode => Self::Unicode,
            ast::str_prefix::StringLiteralPrefix::Raw { uppercase } => Self::Raw { uppercase },
        }
    }
}

impl From<ast::str_prefix::ByteStringPrefix> for BytesPrefix {
    fn from(prefix: ast::str_prefix::ByteStringPrefix) -> Self {
        match prefix {
            ast::str_prefix::ByteStringPrefix::Regular => Self::Regular,
            ast::str_prefix::ByteStringPrefix::Raw { uppercase_r } => Self::Raw { uppercase_r },
        }
    }
}

impl From<ast::str_prefix::FStringPrefix> for FStringPrefix {
    fn from(prefix: ast::str_prefix::FStringPrefix) -> Self {
        match prefix {
            ast::str_prefix::FStringPrefix::Regular => Self::Regular,
            ast::str_prefix::FStringPrefix::Raw { uppercase_r } => Self::Raw { uppercase_r },
        }
    }
}

impl From<ast::str_prefix::TStringPrefix> for TStringPrefix {
    fn from(prefix: ast::str_prefix::TStringPrefix) -> Self {
        match prefix {
            ast::str_prefix::TStringPrefix::Regular => Self::Regular,
            ast::str_prefix::TStringPrefix::Raw { uppercase_r } => Self::Raw { uppercase_r },
        }
    }
}

impl From<&ast::DebugText> for DebugText {
    fn from(text: &ast::DebugText) -> Self {
        Self {
            leading: text.leading().to_owned(),
            expression: text.expression().to_owned(),
            trailing: text.trailing().to_owned(),
        }
    }
}

impl From<ast::ConversionFlag> for Conversion {
    fn from(flag: ast::ConversionFlag) -> Self {
        match flag {
            ast::ConversionFlag::None => Self::None,
            ast::ConversionFlag::Str => Self::Str,
            ast::ConversionFlag::Ascii => Self::Ascii,
            ast::ConversionFlag::Repr => Self::Repr,
        }
    }
}

impl From<&ast::Number> for Number {
    fn from(number: &ast::Number) -> Self {
        match number {
            ast::Number::Int(int) => Self::Int {
                value: int.to_string(),
            },
            ast::Number::Float(value) => Self::Float { value: *value },
            ast::Number::Complex { real, imag } => Self::Complex {
                real: *real,
                imag: *imag,
            },
        }
    }
}

impl From<ast::BoolOp> for BoolOp {
    fn from(op: ast::BoolOp) -> Self {
        match op {
            ast::BoolOp::And => Self::And,
            ast::BoolOp::Or => Self::Or,
        }
    }
}

impl From<ast::Operator> for Operator {
    fn from(op: ast::Operator) -> Self {
        match op {
            ast::Operator::Add => Self::Add,
            ast::Operator::Sub => Self::Sub,
            ast::Operator::Mult => Self::Mult,
            ast::Operator::MatMult => Self::MatMult,
            ast::Operator::Div => Self::Div,
            ast::Operator::Mod => Self::Mod,
            ast::Operator::Pow => Self::Pow,
            ast::Operator::LShift => Self::LShift,
            ast::Operator::RShift => Self::RShift,
            ast::Operator::BitOr => Self::BitOr,
            ast::Operator::BitXor => Self::BitXor,
            ast::Operator::BitAnd => Self::BitAnd,
            ast::Operator::FloorDiv => Self::FloorDiv,
        }
    }
}

impl From<ast::UnaryOp> for UnaryOp {
    fn from(op: ast::UnaryOp) -> Self {
        match op {
            ast::UnaryOp::Invert => Self::Invert,
            ast::UnaryOp::Not => Self::Not,
            ast::UnaryOp::UAdd => Self::UAdd,
            ast::UnaryOp::USub => Self::USub,
        }
    }
}

impl From<ast::CmpOp> for CmpOp {
    fn from(op: ast::CmpOp) -> Self {
        match op {
            ast::CmpOp::Eq => Self::Eq,
            ast::CmpOp::NotEq => Self::NotEq,
            ast::CmpOp::Lt => Self::Lt,
            ast::CmpOp::LtE => Self::LtE,
            ast::CmpOp::Gt => Self::Gt,
            ast::CmpOp::GtE => Self::GtE,
            ast::CmpOp::Is => Self::Is,
            ast::CmpOp::IsNot => Self::IsNot,
            ast::CmpOp::In => Self::In,
            ast::CmpOp::NotIn => Self::NotIn,
        }
    }
}

/// How tightly an expression binds, loosest first, after the Python grammar.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Precedence {
    Yield,
    Named,
    Lambda,
    IfElse,
    Or,
    And,
    Not,
    Compare,
    BitOr,
    BitXor,
    BitAnd,
    Shift,
    Arith,
    Term,
    Factor,
    Power,
    Await,
    Atom,
}

impl Precedence {
    fn tighter(self) -> Self {
        match self {
            Self::Yield => Self::Named,
            Self::Named => Self::Lambda,
            Self::Lambda => Self::IfElse,
            Self::IfElse => Self::Or,
            Self::Or => Self::And,
            Self::And => Self::Not,
            Self::Not => Self::Compare,
            Self::Compare => Self::BitOr,
            Self::BitOr => Self::BitXor,
            Self::BitXor => Self::BitAnd,
            Self::BitAnd => Self::Shift,
            Self::Shift => Self::Arith,
            Self::Arith => Self::Term,
            Self::Term => Self::Factor,
            Self::Factor => Self::Power,
            Self::Power => Self::Await,
            Self::Await | Self::Atom => Self::Atom,
        }
    }
}

impl Expr {
    fn precedence(&self) -> Precedence {
        match self {
            Self::Yield { .. } | Self::YieldFrom { .. } => Precedence::Yield,
            Self::Named { .. } => Precedence::Named,
            Self::Lambda { .. } => Precedence::Lambda,
            Self::If { .. } => Precedence::IfElse,
            Self::BoolOp { op, .. } => op.precedence(),
            Self::UnaryOp { op, .. } => op.precedence(),
            Self::Compare { .. } => Precedence::Compare,
            Self::BinOp { op, .. } => op.precedence(),
            Self::Await { .. } => Precedence::Await,
            // Displays print their own brackets, and a starred item or a slice appears only
            // where the grammar takes it bare.
            Self::Dict { .. }
            | Self::Set { .. }
            | Self::ListComp { .. }
            | Self::SetComp { .. }
            | Self::DictComp { .. }
            | Self::Generator { .. }
            | Self::Call { .. }
            | Self::FString { .. }
            | Self::TString { .. }
            | Self::StringLiteral { .. }
            | Self::BytesLiteral { .. }
            | Self::NumberLiteral { .. }
            | Self::BooleanLiteral { .. }
            | Self::NoneLiteral
            | Self::EllipsisLiteral
            | Self::Attribute { .. }
            | Self::Subscript { .. }
            | Self::Starred { .. }
            | Self::Name { .. }
            | Self::List { .. }
            | Self::Tuple { .. }
            | Self::Slice { .. } => Precedence::Atom,
        }
    }
}

impl BoolOp {
    fn precedence(self) -> Precedence {
        match self {
            Self::Or => Precedence::Or,
            Self::And => Precedence::And,
        }
    }

    fn text(self) -> &'static str {
        match self {
            Self::And => "and",
            Self::Or => "or",
        }
    }
}

impl Operator {
    fn precedence(self) -> Precedence {
        match self {
            Self::BitOr => Precedence::BitOr,
            Self::BitXor => Precedence::BitXor,
            Self::BitAnd => Precedence::BitAnd,
            Self::LShift | Self::RShift => Precedence::Shift,
            Self::Add | Self::Sub => Precedence::Arith,
            Self::Mult | Self::MatMult | Self::Div | Self::FloorDiv | Self::Mod => Precedence::Term,
            Self::Pow => Precedence::Power,
        }
    }

    fn text(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mult => "*",
            Self::MatMult => "@",
            Self::Div => "/",
            Self::Mod => "%",
            Self::Pow => "**",
            Self::LShift => "<<",
            Self::RShift => ">>",
            Self::BitOr => "|",
            Self::BitXor => "^",
            Self::BitAnd => "&",
            Self::FloorDiv => "//",
        }
    }
}

impl UnaryOp {
    fn precedence(self) -> Precedence {
        match self {
            Self::Not => Precedence::Not,
            Self::Invert | Self::UAdd | Self::USub => Precedence::Factor,
        }
    }

    fn text(self) -> &'static str {
        match self {
            Self::Invert => "~",
            Self::Not => "not ",
            Self::UAdd => "+",
            Self::USub => "-",
        }
    }
}

impl CmpOp {
    fn text(self) -> &'static str {
        match self {
            Self::Eq => "==",
            Self::NotEq => "!=",
            Self::Lt => "<",
            Self::LtE => "<=",
            Self::Gt => ">",
            Self::GtE => ">=",
            Self::Is => "is",
            Self::IsNot => "is not",
            Self::In => "in",
            Self::NotIn => "not in",
        }
    }
}

impl Conversion {
    fn text(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Str => Some("s"),
            Self::Ascii => Some("a"),
            Self::Repr => Some("r"),
        }
    }
}

/// Prints the model as Python source, adding the parentheses that precedence requires.
#[derive(Default)]
struct Printer {
    text: String,
    /// The classes enclosing the current statement.
    indent: usize,
}

impl Printer {
    /// Starts a statement's line at the current indentation.
    fn indentation(&mut self) {
        for _ in 0..self.indent {
            self.push("    ");
        }
    }

    fn push(&mut self, text: &str) {
        self.text.push_str(text);
    }

    /// `items` printed by `each`, comma-separated.
    fn join<T>(&mut self, items: &[T], each: impl FnMut(&mut Self, &T)) {
        self.separated(items, ", ", each);
    }

    /// `items` printed by `each`, `separator` between them.
    fn separated<T>(&mut self, items: &[T], separator: &str, mut each: impl FnMut(&mut Self, &T)) {
        for (index, item) in items.iter().enumerate() {
            if index > 0 {
                self.push(separator);
            }
            each(self, item);
        }
    }

    /// Pushes the comma that separates a list item from the ones before it.
    fn separate(&mut self, first: &mut bool) {
        if !*first {
            self.push(", ");
        }
        *first = false;
    }

    /// The function's decorators, then `def name[...](params) -> R:`.
    fn function_header(&mut self, function: &Function) {
        self.decorators(&function.decorators);
        self.indentation();
        if function.is_async {
            self.push("async ");
        }
        self.push("def ");
        self.push(&function.name);
        self.type_params(&function.type_params);
        self.push("(");
        self.parameters(&function.parameters);
        self.push(")");
        if let Some(returns) = &function.returns {
            self.push(" -> ");
            self.expr(returns, Precedence::Lambda);
        }
        self.push(":");
    }

    /// The class's decorators, then `class Name[...](bases, keywords):`.
    fn class_header(&mut self, class: &Class) {
        self.decorators(&class.decorators);
        self.indentation();
        self.push("class ");
        self.push(&class.name);
        self.type_params(&class.type_params);
        if !class.bases.is_empty() || !class.keywords.is_empty() {
            self.arguments(&class.bases, &class.keywords);
        }
        self.push(":");
    }

    fn decorators(&mut self, decorators: &[Expr]) {
        for decorator in decorators {
            self.indentation();
            self.push("@");
            self.expr(decorator, Precedence::Named);
            self.push("\n");
        }
    }

    fn attribute(&mut self, attribute: &Attribute) {
        self.indentation();
        match attribute {
            Attribute::AnnAssign {
                target,
                annotation,
                value,
            } => {
                self.expr(target, Precedence::Atom);
                self.push(": ");
                self.expr(annotation, Precedence::Lambda);
                if let Some(value) = value {
                    self.push(" = ");
                    self.expr(value, Precedence::Lambda);
                }
            }
            Attribute::Assign { targets, value } => {
                for target in targets {
                    self.expr(target, Precedence::Atom);
                    self.push(" = ");
                }
                self.expr(value, Precedence::Lambda);
            }
            Attribute::TypeAlias {
                name,
                type_params,
                value,
            } => {
                self.push("type ");
                self.expr(name, Precedence::Atom);
                self.type_params(type_params);
                self.push(" = ");
                self.expr(value, Precedence::Lambda);
            }
        }
        self.push("\n");
    }

    fn type_params(&mut self, params: &[TypeParam]) {
        if params.is_empty() {
            return;
        }
        self.push("[");
        self.join(params, Self::type_param);
        self.push("]");
    }

    fn type_param(&mut self, param: &TypeParam) {
        let default = match param {
            TypeParam::TypeVar {
                name,
                bound,
                default,
            } => {
                self.push(name);
                if let Some(bound) = bound {
                    self.push(": ");
                    self.expr(bound, Precedence::Lambda);
                }
                default
            }
            TypeParam::ParamSpec { name, default } => {
                self.push("**");
                self.push(name);
                default
            }
            TypeParam::TypeVarTuple { name, default } => {
                self.push("*");
                self.push(name);
                default
            }
        };
        if let Some(default) = default {
            self.push(" = ");
            self.expr(default, Precedence::Lambda);
        }
    }

    fn parameters(&mut self, parameters: &Parameters) {
        let mut first = true;
        for parameter in &parameters.posonly {
            self.separate(&mut first);
            self.parameter(parameter);
        }
        if !parameters.posonly.is_empty() {
            self.separate(&mut first);
            self.push("/");
        }
        for parameter in &parameters.args {
            self.separate(&mut first);
            self.parameter(parameter);
        }
        match &parameters.vararg {
            Some(vararg) => {
                self.separate(&mut first);
                self.push("*");
                self.variadic(vararg);
            }
            None if !parameters.kwonly.is_empty() => {
                self.separate(&mut first);
                self.push("*");
            }
            None => {}
        }
        for parameter in &parameters.kwonly {
            self.separate(&mut first);
            self.parameter(parameter);
        }
        if let Some(kwarg) = &parameters.kwarg {
            self.separate(&mut first);
            self.push("**");
            self.variadic(kwarg);
        }
    }

    fn parameter(&mut self, parameter: &Parameter) {
        self.push(&parameter.name);
        if let Some(annotation) = &parameter.annotation {
            self.push(": ");
            self.expr(annotation, Precedence::Lambda);
        }
        if let Some(default) = &parameter.default {
            self.push(match parameter.annotation {
                Some(_) => " = ",
                None => "=",
            });
            self.expr(default, Precedence::Lambda);
        }
    }

    fn variadic(&mut self, variadic: &Variadic) {
        self.push(&variadic.name);
        if let Some(annotation) = &variadic.annotation {
            self.push(": ");
            self.expr(annotation, Precedence::Lambda);
        }
    }

    /// `(args, keywords)` of a call or class header.
    fn arguments(&mut self, args: &[Expr], keywords: &[Keyword]) {
        self.push("(");
        match (args, keywords) {
            // A lone generator argument shares the call's parentheses.
            ([Expr::Generator { elt, generators }], []) => {
                self.expr(elt, Precedence::Named);
                self.generators(generators);
            }
            _ => {
                self.join(args, |printer, arg| printer.expr(arg, Precedence::Named));
                if !args.is_empty() && !keywords.is_empty() {
                    self.push(", ");
                }
                self.join(keywords, Self::keyword);
            }
        }
        self.push(")");
    }

    fn keyword(&mut self, keyword: &Keyword) {
        match &keyword.arg {
            Some(arg) => {
                self.push(arg);
                self.push("=");
                self.expr(&keyword.value, Precedence::Lambda);
            }
            None => {
                self.push("**");
                self.expr(&keyword.value, Precedence::BitOr);
            }
        }
    }

    fn generators(&mut self, generators: &[Comprehension]) {
        for generator in generators {
            self.push(if generator.is_async {
                " async for "
            } else {
                " for "
            });
            self.expr(&generator.target, Precedence::Or);
            self.push(" in ");
            self.expr(&generator.iter, Precedence::Or);
            for condition in &generator.ifs {
                self.push(" if ");
                self.expr(condition, Precedence::Or);
            }
        }
    }

    /// `expr`, parenthesized if it binds more loosely than `min`.
    fn expr(&mut self, expr: &Expr, min: Precedence) {
        let parenthesize = expr.precedence() < min;
        if parenthesize {
            self.push("(");
        }
        self.bare(expr);
        if parenthesize {
            self.push(")");
        }
    }

    fn bare(&mut self, expr: &Expr) {
        match expr {
            Expr::BoolOp { op, values } => {
                let operand = op.precedence().tighter();
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        self.push(" ");
                        self.push(op.text());
                        self.push(" ");
                    }
                    self.expr(value, operand);
                }
            }
            Expr::Named { target, value } => {
                self.expr(target, Precedence::Atom);
                self.push(" := ");
                self.expr(value, Precedence::Lambda);
            }
            Expr::BinOp { left, op, right } => {
                // `**` binds right to left, and its right operand may be unary.
                let (left_min, right_min) = if *op == Operator::Pow {
                    (Precedence::Await, Precedence::Factor)
                } else {
                    (op.precedence(), op.precedence().tighter())
                };
                self.expr(left, left_min);
                self.push(" ");
                self.push(op.text());
                self.push(" ");
                self.expr(right, right_min);
            }
            Expr::UnaryOp { op, operand } => {
                self.push(op.text());
                self.expr(operand, op.precedence());
            }
            Expr::Lambda { parameters, body } => {
                self.push("lambda");
                if !parameters.is_empty() {
                    self.push(" ");
                    self.parameters(parameters);
                }
                self.push(": ");
                self.expr(body, Precedence::Lambda);
            }
            Expr::If { test, body, orelse } => {
                self.expr(body, Precedence::Or);
                self.push(" if ");
                self.expr(test, Precedence::Or);
                self.push(" else ");
                self.expr(orelse, Precedence::Lambda);
            }
            Expr::Dict { items } => {
                self.push("{");
                self.join(items, |printer, item| match &item.key {
                    Some(key) => {
                        printer.expr(key, Precedence::IfElse);
                        printer.push(": ");
                        printer.expr(&item.value, Precedence::Lambda);
                    }
                    None => {
                        printer.push("**");
                        printer.expr(&item.value, Precedence::BitOr);
                    }
                });
                self.push("}");
            }
            Expr::Set { elts } => {
                self.push("{");
                self.join(elts, |printer, elt| printer.expr(elt, Precedence::Named));
                self.push("}");
            }
            Expr::ListComp { elt, generators } => {
                self.push("[");
                self.expr(elt, Precedence::Named);
                self.generators(generators);
                self.push("]");
            }
            Expr::SetComp { elt, generators } => {
                self.push("{");
                self.expr(elt, Precedence::Named);
                self.generators(generators);
                self.push("}");
            }
            Expr::DictComp {
                key,
                value,
                generators,
            } => {
                self.push("{");
                match key {
                    Some(key) => {
                        self.expr(key, Precedence::IfElse);
                        self.push(": ");
                        self.expr(value, Precedence::Lambda);
                    }
                    None => {
                        self.push("**");
                        self.expr(value, Precedence::BitOr);
                    }
                }
                self.generators(generators);
                self.push("}");
            }
            Expr::Generator { elt, generators } => {
                self.push("(");
                self.expr(elt, Precedence::Named);
                self.generators(generators);
                self.push(")");
            }
            Expr::Await { value } => {
                self.push("await ");
                self.expr(value, Precedence::Atom);
            }
            Expr::Yield { value } => {
                self.push("yield");
                if let Some(value) = value {
                    self.push(" ");
                    self.expr(value, Precedence::Lambda);
                }
            }
            Expr::YieldFrom { value } => {
                self.push("yield from ");
                self.expr(value, Precedence::Lambda);
            }
            Expr::Compare { left, comparisons } => {
                self.expr(left, Precedence::BitOr);
                for comparison in comparisons {
                    self.push(" ");
                    self.push(comparison.op.text());
                    self.push(" ");
                    self.expr(&comparison.right, Precedence::BitOr);
                }
            }
            Expr::Call {
                func,
                args,
                keywords,
            } => {
                self.expr(func, Precedence::Atom);
                self.arguments(args, keywords);
            }
            Expr::FString { parts } => self.separated(parts, " ", |printer, part| match part {
                FStringPart::Literal(part) => printer.string(part),
                FStringPart::FString(part) => printer.interpolated(&part.flags, &part.elements),
            }),
            Expr::TString { parts } => self.separated(parts, " ", |printer, part| {
                printer.interpolated(&part.flags, &part.elements)
            }),
            Expr::StringLiteral { parts } => self.separated(parts, " ", Self::string),
            Expr::BytesLiteral { parts } => self.separated(parts, " ", Self::bytes),
            Expr::NumberLiteral { value } => self.number(value),
            Expr::BooleanLiteral { value } => self.push(if *value { "True" } else { "False" }),
            Expr::NoneLiteral => self.push("None"),
            Expr::EllipsisLiteral => self.push("..."),
            Expr::Attribute { value, attr } => {
                match &**value {
                    // `1.real` lexes as a float followed by a name.
                    Expr::NumberLiteral {
                        value: number @ Number::Int { .. },
                    } => {
                        self.push("(");
                        self.number(number);
                        self.push(")");
                    }
                    value => self.expr(value, Precedence::Atom),
                }
                self.push(".");
                self.push(attr);
            }
            Expr::Subscript { value, slice } => {
                self.expr(value, Precedence::Atom);
                self.push("[");
                match &**slice {
                    // A subscript's tuple takes no parentheses of its own.
                    Expr::Tuple { elts } if !elts.is_empty() => {
                        self.join(elts, |printer, elt| printer.expr(elt, Precedence::Named));
                        if elts.len() == 1 {
                            self.push(",");
                        }
                    }
                    slice => self.expr(slice, Precedence::Named),
                }
                self.push("]");
            }
            Expr::Starred { value } => {
                self.push("*");
                self.expr(value, Precedence::BitOr);
            }
            Expr::Name { id } => self.push(id),
            Expr::List { elts } => {
                self.push("[");
                self.join(elts, |printer, elt| printer.expr(elt, Precedence::Named));
                self.push("]");
            }
            Expr::Tuple { elts } => {
                self.push("(");
                self.join(elts, |printer, elt| printer.expr(elt, Precedence::Named));
                if elts.len() == 1 {
                    self.push(",");
                }
                self.push(")");
            }
            Expr::Slice { lower, upper, step } => {
                if let Some(lower) = lower {
                    self.expr(lower, Precedence::IfElse);
                }
                self.push(":");
                if let Some(upper) = upper {
                    self.expr(upper, Precedence::IfElse);
                }
                if let Some(step) = step {
                    self.push(":");
                    self.expr(step, Precedence::IfElse);
                }
            }
        }
    }

    // Debug formatting prints the shortest float that reads back the same, with a `.0` or an
    // exponent, so it never reads as an int.
    fn number(&mut self, number: &Number) {
        match number {
            Number::Int { value } => self.push(value),
            Number::Float { value } => self.push(&format!("{value:?}")),
            Number::Complex { real: _, imag } => self.push(&format!("{imag:?}j")),
        }
    }

    fn string(&mut self, part: &StringPart) {
        let quote = part.flags.quote();
        let raw = part.flags.is_raw();
        self.open(&part.flags);
        for ch in part.value.chars() {
            push_char(&mut self.text, ch, quote, raw);
        }
        self.quotes(&part.flags);
    }

    fn bytes(&mut self, part: &BytesPart) {
        let quote = part.flags.quote();
        let raw = part.flags.is_raw();
        self.open(&part.flags);
        for &byte in &part.value {
            match byte {
                // A raw literal's value is its source text, which is ASCII.
                byte if raw => self.text.push(char::from(byte)),
                b'\\' => self.push("\\\\"),
                b'\n' => self.push("\\n"),
                b'\r' => self.push("\\r"),
                b'\t' => self.push("\\t"),
                byte if char::from(byte) == quote => {
                    self.text.push('\\');
                    self.text.push(quote);
                }
                b' '..=b'~' => self.text.push(char::from(byte)),
                byte => self.push(&format!("\\x{byte:02x}")),
            }
        }
        self.quotes(&part.flags);
    }

    /// An f-string or t-string part.
    fn interpolated<P: Prefix>(&mut self, flags: &Flags<P>, elements: &[Element]) {
        self.open(flags);
        self.elements(elements, flags.quote(), flags.is_raw());
        self.quotes(flags);
    }

    fn open<P: Prefix>(&mut self, flags: &Flags<P>) {
        self.push(flags.prefix.text());
        self.quotes(flags);
    }

    fn quotes<P: Prefix>(&mut self, flags: &Flags<P>) {
        let count = if flags.triple_quoted { 3 } else { 1 };
        for _ in 0..count {
            self.text.push(flags.quote());
        }
    }

    fn elements(&mut self, elements: &[Element], quote: char, raw: bool) {
        for element in elements {
            match element {
                Element::Literal { value } => {
                    for ch in value.chars() {
                        match ch {
                            '{' | '}' => {
                                self.text.push(ch);
                                self.text.push(ch);
                            }
                            ch => push_char(&mut self.text, ch, quote, raw),
                        }
                    }
                }
                Element::Interpolation {
                    expression,
                    debug_text,
                    conversion,
                    format_spec,
                } => {
                    self.push("{");
                    match debug_text {
                        // The source text is part of the string's value, so it prints as written.
                        Some(debug_text) => {
                            self.push(&debug_text.leading);
                            self.push(&debug_text.expression);
                            self.push(&debug_text.trailing);
                        }
                        None => self.field(expression),
                    }
                    if let Some(conversion) = conversion.text() {
                        self.push("!");
                        self.push(conversion);
                    }
                    if let Some(format_spec) = format_spec {
                        self.push(":");
                        self.elements(format_spec, quote, raw);
                    }
                    self.push("}");
                }
            }
        }
    }

    /// The expression of a replacement field, spaced from the field's braces where its own
    /// would otherwise double them.
    fn field(&mut self, expression: &Expr) {
        let start = self.text.len();
        self.expr(expression, Precedence::IfElse);
        if self.text[start..].starts_with('{') {
            self.text.insert(start, ' ');
        }
        if self.text.ends_with('}') {
            self.text.push(' ');
        }
    }
}

/// Pushes `ch` of a decoded value as it reads in a literal between `quote`s: as is in a raw
/// literal, whose value is its source text, else escaped where needed.
fn push_char(text: &mut String, ch: char, quote: char, raw: bool) {
    match ch {
        ch if raw => text.push(ch),
        '\\' => text.push_str("\\\\"),
        '\n' => text.push_str("\\n"),
        '\r' => text.push_str("\\r"),
        '\t' => text.push_str("\\t"),
        ch if ch == quote => {
            text.push('\\');
            text.push(ch);
        }
        // Every control character is in Latin-1.
        ch if ch.is_control() => text.push_str(&format!("\\x{:02x}", u32::from(ch))),
        ch => text.push(ch),
    }
}
