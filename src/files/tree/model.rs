use std::{collections::HashSet, path::PathBuf};

use ely_gpui_component::data_display::Tone;
use gpui::SharedString;

/// Where a dragged node lands against a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropAt {
    Before,
    Inside,
    After,
}

/// A drag of one tree's node, named by its owner.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeDrag {
    pub owner: gpui::EntityId,
    pub path: PathBuf,
    pub key: SharedString,
    pub label: SharedString,
}

/// What a node holds under it.
#[derive(Clone, Debug, PartialEq)]
pub enum Children {
    Leaf,
    Known(Vec<TreeNode>),
    Pending,
}

/// One node of a tree: its key, its label, an icon, and its children.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeNode {
    pub key: SharedString,
    pub path: PathBuf,
    pub label: SharedString,
    pub icon: Option<ely_gpui_component::primitives::IconName>,
    pub note: Option<SharedString>,
    pub tone: Option<Tone>,
    pub children: Children,
}

impl TreeNode {
    /// A leaf, until it gains children.
    pub fn new(
        key: impl Into<SharedString>,
        path: PathBuf,
        label: impl Into<SharedString>,
    ) -> Self {
        Self {
            key: key.into(),
            path,
            label: label.into(),
            icon: None,
            note: None,
            tone: None,
            children: Children::Leaf,
        }
    }

    /// Quiet text at the row's end, such as a status letter.
    pub fn note(mut self, note: impl Into<SharedString>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// Tints the label and the note.
    pub fn tone(mut self, tone: impl Into<Tone>) -> Self {
        self.tone = Some(tone.into());
        self
    }

    pub fn icon(mut self, icon: ely_gpui_component::primitives::IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Makes it a node that opens, even with no children yet.
    pub fn children(mut self, nodes: impl IntoIterator<Item = TreeNode>) -> Self {
        let mut known = match self.children {
            Children::Known(known) => known,
            _ => Vec::new(),
        };
        known.extend(nodes);
        self.children = Children::Known(known);
        self
    }

    /// Children still to come: opening it runs `on_load`.
    pub fn pending(mut self) -> Self {
        self.children = Children::Pending;
        self
    }

    pub fn opens(&self) -> bool {
        self.children != Children::Leaf
    }

    fn holds(&self, key: &SharedString) -> bool {
        self.key == *key
            || matches!(&self.children, Children::Known(children) if children.iter().any(|c| c.holds(key)))
    }
}

/// Finds a node by key.
pub fn find<'a>(nodes: &'a [TreeNode], key: &SharedString) -> Option<&'a TreeNode> {
    nodes.iter().find_map(|node| {
        if node.key == *key {
            return Some(node);
        }
        match &node.children {
            Children::Known(children) => find(children, key),
            _ => None,
        }
    })
}

pub fn can_drop(nodes: &[TreeNode], dragged: &SharedString, target: &SharedString) -> bool {
    find(nodes, dragged).is_some_and(|node| !node.holds(target))
}

pub fn place_at(share: f32, opens: bool) -> DropAt {
    match (share, opens) {
        (share, true) if (0.25..0.75).contains(&share) => DropAt::Inside,
        (share, _) if share < 0.5 => DropAt::Before,
        _ => DropAt::After,
    }
}

/// What a row shows: a node, or the wait for a pending node's children.
#[derive(Clone, Debug, PartialEq)]
pub enum Shown {
    Node {
        key: SharedString,
        path: PathBuf,
        label: SharedString,
        icon: Option<ely_gpui_component::primitives::IconName>,
        note: Option<SharedString>,
        tone: Option<Tone>,
        opens: bool,
        open: bool,
    },
    Loading,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub shown: Shown,
    pub depth: usize,
}

impl Row {
    pub fn key(&self) -> Option<&SharedString> {
        match &self.shown {
            Shown::Node { key, .. } => Some(key),
            Shown::Loading => None,
        }
    }
}

pub fn rows(nodes: &[TreeNode], open: &HashSet<SharedString>) -> Vec<Row> {
    fn walk(nodes: &[TreeNode], open: &HashSet<SharedString>, depth: usize, out: &mut Vec<Row>) {
        for node in nodes {
            let is_open = node.opens() && open.contains(&node.key);
            out.push(Row {
                shown: Shown::Node {
                    key: node.key.clone(),
                    path: node.path.clone(),
                    label: node.label.clone(),
                    icon: node.icon,
                    note: node.note.clone(),
                    tone: node.tone,
                    opens: node.opens(),
                    open: is_open,
                },
                depth,
            });
            match (&node.children, is_open) {
                (Children::Known(children), true) => walk(children, open, depth + 1, out),
                (Children::Pending, true) => {
                    out.push(Row {
                        shown: Shown::Loading,
                        depth: depth + 1,
                    });
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(nodes, open, 0, &mut out);
    out
}
