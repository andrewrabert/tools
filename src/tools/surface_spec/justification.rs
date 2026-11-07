//! Justifications of a capture's nodes: one sentence per node, kept in the capture's JSON.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result, anyhow, bail};
use clap::Subcommand;
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
                (file.every_declaration()?, Some(file.language()))
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

    fn place(&self, id: CaptureId) -> Result<&Place> {
        self.places
            .get(&id)
            .ok_or_else(|| anyhow!("node {id} is not among the nodes"))
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
