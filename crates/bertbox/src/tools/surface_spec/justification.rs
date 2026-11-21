//! Justifications of a capture's nodes: one sentence per node, kept in the capture's JSON.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{CaptureId, CaptureJson, CapturedSurface, Children, Errors, Language, NodeKind, write};

#[derive(Subcommand)]
pub(super) enum Command {
    /// List every node of a capture with its parent and whether it is justified
    Parts {
        /// JSON capture
        #[arg(value_name = "CAPTURE")]
        capture: PathBuf,
    },
    /// Print the input that justifying one node needs
    Context {
        /// JSON capture
        #[arg(value_name = "CAPTURE")]
        capture: PathBuf,
        /// Node id to print the input of
        #[arg(value_name = "NODE")]
        node: CaptureId,
        /// Print the input as JSON instead of Markdown
        #[arg(long)]
        json: bool,
    },
    /// Write the justification of one node into a capture
    Justify {
        /// JSON capture to rewrite
        #[arg(value_name = "CAPTURE")]
        capture: PathBuf,
        /// Node id to justify
        #[arg(value_name = "NODE")]
        node: CaptureId,
        /// The one-sentence justification
        #[arg(value_name = "SENTENCE")]
        sentence: Justification,
    },
    /// List nodes whose justification is missing
    Check {
        /// JSON capture
        #[arg(value_name = "CAPTURE")]
        capture: PathBuf,
    },
    /// Print the justification or code of each node, after the ids of the nodes on the way to it
    Tree(Box<TreeArgs>),
}

/// What `tree` prints after each node's ids.
#[derive(Clone, Copy, ValueEnum)]
pub(super) enum TreeFormat {
    /// The node's justification
    Justification,
    /// Each line of the node's surface spec
    Code,
}

/// Boxed, so that `tree`'s arguments don't grow every tool's.
#[derive(Args)]
pub(super) struct TreeArgs {
    /// JSON capture
    #[arg(value_name = "CAPTURE")]
    capture: PathBuf,
    /// Node id to start the tree at [default: the root]
    #[arg(long, value_name = "ID")]
    id: Option<CaptureId>,
    /// Levels of parents above the start to print
    #[arg(short, long, value_name = "N", default_value_t = 0)]
    parent_depth: usize,
    /// What to print after each node's ids
    #[arg(short, long, value_enum, default_value_t = TreeFormat::Justification)]
    format: TreeFormat,
}

impl Command {
    pub(super) fn run(self) -> Result<(), Errors> {
        match self {
            Self::Parts { capture } => Capture::load(&capture)?.parts(),
            Self::Context {
                capture,
                node,
                json,
            } => {
                let format = if json { Format::Json } else { Format::Markdown };
                Capture::load(&capture)?.context(node, format)
            }
            Self::Justify {
                capture: path,
                node,
                sentence,
            } => {
                let mut capture = Capture::load(&path)?;
                capture.justify(node, sentence)?;
                capture.save(&path)
            }
            Self::Check { capture } => Capture::load(&capture)?.check(),
            Self::Tree(args) => {
                let TreeArgs {
                    capture,
                    id,
                    parent_depth,
                    format,
                } = *args;
                let capture = Capture::load(&capture)?;
                capture.write_tree(
                    id.unwrap_or(capture.json.root),
                    parent_depth,
                    format,
                    &mut io::BufWriter::new(io::stdout().lock()),
                )
            }
        }
        .map_err(Errors::from)
    }
}

/// One sentence on why a node exists: one non-empty line.
#[derive(Clone, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(super) struct Justification(String);

/// Why a string is not a `Justification`.
#[derive(Debug)]
pub(super) enum JustificationError {
    Blank,
    MultipleLines,
}

impl fmt::Display for JustificationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            Self::Blank => "a justification needs text",
            Self::MultipleLines => "a justification is one line",
        })
    }
}

impl std::error::Error for JustificationError {}

impl TryFrom<String> for Justification {
    type Error = JustificationError;

    fn try_from(sentence: String) -> Result<Self, JustificationError> {
        if sentence.trim().is_empty() {
            Err(JustificationError::Blank)
        } else if sentence.contains(['\n', '\r']) {
            Err(JustificationError::MultipleLines)
        } else {
            Ok(Self(sentence))
        }
    }
}

impl FromStr for Justification {
    type Err = JustificationError;

    fn from_str(sentence: &str) -> Result<Self, JustificationError> {
        Self::try_from(sentence.to_owned())
    }
}

impl From<Justification> for String {
    fn from(justification: Justification) -> Self {
        justification.0
    }
}

impl AsRef<str> for Justification {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// How `context` prints a node's input.
enum Format {
    Markdown,
    Json,
}

/// A capture's JSON beside the place of each node in its tree.
struct Capture {
    json: CaptureJson,
    /// Every node in tree order, each before the nodes it encloses.
    order: Vec<CaptureId>,
    places: BTreeMap<CaptureId, Place>,
}

/// Where a node sits in its capture's tree.
struct Place {
    parent: Option<CaptureId>,
    kind: String,
    /// The path of the node, or of the file holding it.
    path: PathBuf,
    holder: Holder,
}

/// What a node is in its capture's tree.
enum Holder {
    /// The path or a directory, holding these names of directories and files.
    Dir(Vec<String>),
    File,
    /// A declaration of this file node.
    Declaration(CaptureId),
}

impl Capture {
    /// The capture at `path`, which must hold a well-formed tree and justify only its nodes.
    fn load(path: &Path) -> Result<Self> {
        let text =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("parsing {}", path.display()))
    }

    fn parse(text: &str) -> Result<Self> {
        CapturedSurface::parse(text, false)?;
        let json: CaptureJson = serde_json::from_str(text)?;
        if let Some(id) = json
            .justifications
            .keys()
            .find(|id| !json.nodes.contains_key(id))
        {
            bail!("node {id} is justified but is not among the nodes");
        }
        let mut order = Vec::new();
        let mut places = BTreeMap::new();
        // Nodes to place, beside their parent and the file holding them; the next is last.
        let mut pending = vec![(json.root, None, None)];
        while let Some((id, parent, file)) = pending.pop() {
            let node = &json.nodes[&id];
            let Children { children } = Children::deserialize(node)?;
            let parent_path = parent.map(|parent: CaptureId| places_path(&places, parent));
            let (path, holder, file) = match NodeKind::deserialize(node)? {
                NodeKind::Path(root) => (root.path, Holder::Dir(names(&json, &children)?), None),
                NodeKind::Dir(dir) => (
                    parent_path.unwrap_or_default().join(dir.name),
                    Holder::Dir(names(&json, &children)?),
                    None,
                ),
                NodeKind::File(file) => (
                    parent_path.unwrap_or_default().join(file.name),
                    Holder::File,
                    Some(id),
                ),
                NodeKind::Declaration => {
                    let file = file.ok_or_else(|| anyhow!("node {id} is outside a file"))?;
                    (
                        places_path(&places, file),
                        Holder::Declaration(file),
                        Some(file),
                    )
                }
            };
            let kind = node["kind"]
                .as_str()
                .ok_or_else(|| anyhow!("node {id} has no kind"))?
                .to_owned();
            order.push(id);
            places.insert(
                id,
                Place {
                    parent,
                    kind,
                    path,
                    holder,
                },
            );
            pending.extend(
                children
                    .into_iter()
                    .rev()
                    .map(|child| (child, Some(id), file)),
            );
        }
        Ok(Self {
            json,
            order,
            places,
        })
    }

    fn parts(&self) -> Result<()> {
        let mut stdout = io::stdout().lock();
        for id in &self.order {
            let place = &self.places[id];
            let line = json!({
                "id": id,
                "kind": place.kind,
                "parent": place.parent,
                "justified": self.json.justifications.contains_key(id),
            });
            writeln!(stdout, "{line}").context("writing to stdout")?;
        }
        Ok(())
    }

    /// Prints the node's kind, its parent's kind and justification, and its surface. A node
    /// below the root needs its parent justified first.
    fn context(&self, id: CaptureId, format: Format) -> Result<()> {
        let place = self.place(id)?;
        let parent = match place.parent {
            None => None,
            Some(parent) => {
                let justification = self.json.justifications.get(&parent).ok_or_else(|| {
                    anyhow!("node {id} needs its parent, node {parent}, justified first")
                })?;
                Some((&self.places[&parent].kind, justification))
            }
        };
        let (surface, language) = match &place.holder {
            Holder::Dir(names) => (names.join("\n"), None),
            Holder::File => {
                let (file, _) = self.json.file(id, place.path.clone())?;
                (file.every_declaration()?.text(&file), Some(file.language()))
            }
            Holder::Declaration(file) => {
                let (file, ids) = self.json.file(*file, place.path.clone())?;
                (file.surface(ids[&id])?, Some(file.language()))
            }
        };
        let surface = surface.trim_end();
        let text = match format {
            Format::Json => json!({
                "kind": place.kind,
                "parent": parent.map(|(kind, justification)| json!({
                    "kind": kind,
                    "justification": justification,
                })),
                "surface": surface,
            })
            .to_string(),
            Format::Markdown => {
                let mut text = format!("# {}\n\n", place.kind);
                if let Some((kind, justification)) = parent {
                    text.push_str(&format!(
                        "## Parent\n\n{kind}: {}\n\n",
                        justification.as_ref()
                    ));
                }
                let fence = match language {
                    None => "",
                    Some(Language::Rust) => "rust",
                    Some(Language::Python) => "python",
                };
                text.push_str(&format!("## Surface\n\n```{fence}\n{surface}\n```"));
                text
            }
        };
        writeln!(io::stdout(), "{text}").context("writing to stdout")
    }

    fn justify(&mut self, id: CaptureId, justification: Justification) -> Result<()> {
        self.place(id)?;
        self.json.justifications.insert(id, justification);
        Ok(())
    }

    fn save(&self, path: &Path) -> Result<()> {
        write(path, &format!("{}\n", serde_json::to_string(&self.json)?))
    }

    /// Prints the id of each node without a justification, failing when there is one.
    fn check(&self) -> Result<()> {
        let missing = Vec::from_iter(
            self.order
                .iter()
                .filter(|id| !self.json.justifications.contains_key(id)),
        );
        let mut stdout = io::stdout().lock();
        for id in &missing {
            writeln!(stdout, "{id}").context("writing to stdout")?;
        }
        if !missing.is_empty() {
            bail!(
                "{} of {} nodes have no justification",
                missing.len(),
                self.order.len()
            );
        }
        Ok(())
    }

    /// Writes the nodes `tree_paths` shows, each line starting with the node's path. With
    /// `Justification`, a line per node, in tree order, then its justification, if it has one.
    /// With `Code`, a line per line of the surface spec of each file holding a node shown, then
    /// that line, only for the lines of the nodes shown, all unindented by as much as the least
    /// indented of a file's.
    fn write_tree(
        &self,
        start: CaptureId,
        parents: usize,
        format: TreeFormat,
        out: &mut impl Write,
    ) -> Result<()> {
        let shown = self.tree_paths(start, parents)?;
        match format {
            TreeFormat::Justification => {
                for (id, path) in &shown {
                    match self.json.justifications.get(id) {
                        Some(justification) => writeln!(out, "{path} {}", justification.as_ref()),
                        None => writeln!(out, "{path}"),
                    }
                    .context("writing to stdout")?;
                }
            }
            TreeFormat::Code => {
                let paths = BTreeMap::from_iter(shown.iter().map(|(id, path)| (*id, path)));
                let mut files = Vec::new();
                for (id, _) in &shown {
                    let file = match self.places[id].holder {
                        Holder::Dir(_) => continue,
                        Holder::File => *id,
                        Holder::Declaration(file) => file,
                    };
                    if !files.contains(&file) {
                        files.push(file);
                    }
                }
                for file in files {
                    let lines = Vec::from_iter(
                        self.code_lines(file)?
                            .into_iter()
                            .filter(|(id, _)| paths.contains_key(id)),
                    );
                    let indent = lines
                        .iter()
                        .map(|(_, line)| indentation(line))
                        .min()
                        .unwrap_or(0);
                    for (id, line) in &lines {
                        writeln!(out, "{} {}", paths[id], &line[indent..])
                            .context("writing to stdout")?;
                    }
                }
            }
        }
        out.flush().context("writing to stdout")
    }

    /// Each of the `parents` nearest ancestors of `start`, the highest first, then `start` and
    /// each node below it, in tree order, beside its path: the ids of the nodes from the highest
    /// of them down to it, joined by `:`.
    fn tree_paths(&self, start: CaptureId, parents: usize) -> Result<Vec<(CaptureId, String)>> {
        let begin = self
            .order
            .iter()
            .position(|&id| id == start)
            .ok_or_else(|| anyhow!("node {start} is not among the nodes"))?;
        let mut ancestors = Vec::new();
        let mut next = self.places[&start].parent;
        while let Some(id) = next
            && ancestors.len() < parents
        {
            ancestors.push(id);
            next = self.places[&id].parent;
        }
        let mut shown = Vec::new();
        let mut above = None;
        for &id in ancestors.iter().rev() {
            let path = tree_path(above.as_deref(), id);
            above = Some(path.clone());
            shown.push((id, path));
        }
        // The path of each node below `start` so far. Tree order puts each node after its
        // parent, and the nodes below `start` right after it.
        let mut paths = BTreeMap::<CaptureId, String>::new();
        for &id in &self.order[begin..] {
            let parent = if id == start {
                above.as_deref()
            } else {
                match self.places[&id]
                    .parent
                    .and_then(|parent| paths.get(&parent))
                {
                    Some(parent) => Some(parent.as_str()),
                    None => break,
                }
            };
            let path = tree_path(parent, id);
            paths.insert(id, path.clone());
            shown.push((id, path));
        }
        Ok(shown)
    }

    /// Each non-blank line of the surface spec of file node `file`, beside the node it renders.
    /// A line of no node, like an `extern` block around its fns, is left out.
    fn code_lines(&self, file: CaptureId) -> Result<Vec<(CaptureId, String)>> {
        let (surface, ids) = self.json.file(file, self.places[&file].path.clone())?;
        let capture_ids =
            BTreeMap::from_iter(ids.into_iter().map(|(capture, node)| (node, capture)));
        Ok(surface
            .every_declaration()?
            .node_lines()
            .map(|(node, line)| (capture_ids[&node], line.to_owned()))
            .collect())
    }

    fn place(&self, id: CaptureId) -> Result<&Place> {
        self.places
            .get(&id)
            .ok_or_else(|| anyhow!("node {id} is not among the nodes"))
    }
}

/// The number of spaces `line` starts with.
fn indentation(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// The ids of `parent`'s path, then `id`, joined by `:`.
fn tree_path(parent: Option<&str>, id: CaptureId) -> String {
    match parent {
        Some(parent) => format!("{parent}:{id}"),
        None => id.to_string(),
    }
}

fn places_path(places: &BTreeMap<CaptureId, Place>, id: CaptureId) -> PathBuf {
    places[&id].path.clone()
}

/// The names of the directories and files `children` lists, a directory's ending in `/`.
fn names(json: &CaptureJson, children: &[CaptureId]) -> Result<Vec<String>> {
    children
        .iter()
        .map(|child| {
            Ok(match NodeKind::deserialize(&json.nodes[child])? {
                NodeKind::Dir(dir) => format!("{}/", dir.name.0),
                NodeKind::File(file) => file.name.0,
                NodeKind::Path(_) | NodeKind::Declaration => {
                    bail!("node {child} is not a directory or file")
                }
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_prefixes_each_justification_with_its_ids() {
        let capture = Capture::parse(
            &json!({
                "root": 0,
                "nodes": {
                    "0": {"kind": "path", "path": "/src/p", "children": [1, 3]},
                    "1": {"kind": "dir", "name": "a", "children": [2]},
                    "2": {"kind": "file", "language": "rust", "name": "x.rs"},
                    "3": {"kind": "file", "language": "python", "name": "b.py"},
                },
                "justifications": {"0": "Holds the sources.", "2": "Holds x."},
            })
            .to_string(),
        )
        .expect("capture");
        let tree = |start, parents| {
            let mut out = Vec::new();
            capture
                .write_tree(start, parents, TreeFormat::Justification, &mut out)
                .map(|()| String::from_utf8(out).expect("UTF-8"))
        };
        assert_eq!(
            tree(CaptureId(0), 0).expect("tree"),
            "0 Holds the sources.\n0:1\n0:1:2 Holds x.\n0:3\n",
        );
        assert_eq!(tree(CaptureId(1), 0).expect("tree"), "1\n1:2 Holds x.\n");
        assert_eq!(tree(CaptureId(2), 1).expect("tree"), "1\n1:2 Holds x.\n");
        assert_eq!(
            tree(CaptureId(1), 5).expect("tree"),
            "0 Holds the sources.\n0:1\n0:1:2 Holds x.\n"
        );
        assert!(tree(CaptureId(4), 0).is_err());
    }

    /// `tree -f code` of `source`, captured as the file `path`, from node `start`.
    fn code(path: &str, language: Language, source: &str, start: usize, parents: usize) -> String {
        let surface = CapturedSurface {
            files: vec![
                super::super::FileSurface::parse(PathBuf::from(path), language, source)
                    .expect("capturable source"),
            ],
        };
        let capture =
            Capture::parse(&serde_json::to_string(&surface).expect("JSON")).expect("capture");
        let mut out = Vec::new();
        capture
            .write_tree(CaptureId(start), parents, TreeFormat::Code, &mut out)
            .expect("tree");
        String::from_utf8(out).expect("UTF-8")
    }

    #[test]
    fn tree_code_prefixes_each_line_with_its_nodes_ids() {
        let source = "
class Farts:
    def make_stinky(self) -> str:
        return 'stinky'

    def make_wet(self) -> str:
        return 'wet'
";
        let code =
            |start, parents| code("/src/p/farts.py", Language::Python, source, start, parents);
        assert_eq!(
            code(0, 0),
            "0:1:2 class Farts:\n\
             0:1:2:3     def make_stinky(self) -> str: ...\n\
             0:1:2:4     def make_wet(self) -> str: ...\n",
        );
        assert_eq!(code(3, 0), "3 def make_stinky(self) -> str: ...\n");
        assert_eq!(
            code(3, 1),
            "2 class Farts:\n2:3     def make_stinky(self) -> str: ...\n",
        );
    }

    #[test]
    fn tree_code_gives_closing_lines_to_their_node() {
        let source = "
#[derive(Debug)]
pub struct Error {
    pub encoding: &'static str,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(self.encoding)
    }
}
";
        let code = |start, parents| code("/src/p/x.rs", Language::Rust, source, start, parents);
        assert_eq!(
            code(1, 0),
            "1:2 #[derive(Debug)]\n\
             1:2 pub struct Error {\n\
             1:2:3     pub encoding: &'static str,\n\
             1:2 }\n\
             1:4 impl std::fmt::Display for Error;\n",
        );
        assert_eq!(code(3, 0), "3 pub encoding: &'static str,\n");
        assert_eq!(code(5, 0), "");
    }
}
