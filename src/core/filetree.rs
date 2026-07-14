//! Lazily loaded directory tree model.

use std::path::{Path, PathBuf};

pub struct Node {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub expanded: bool,
    /// `None` = not yet scanned (lazy). `Some` = children loaded.
    pub children: Option<Vec<Node>>,
}

impl Node {
    fn new(path: PathBuf, is_dir: bool) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        Node {
            path,
            name,
            is_dir,
            expanded: false,
            children: None,
        }
    }
}

/// A visible (flattened) tree row — for rendering and selection.
pub struct VisibleRow {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub expanded: bool,
    pub depth: usize,
}

pub struct FileTree {
    pub root: PathBuf,
    pub children: Option<Vec<Node>>,
    /// Selected index among the visible rows.
    pub selected: usize,
}

impl FileTree {
    pub fn new(root: PathBuf) -> Self {
        FileTree {
            root,
            children: None,
            selected: 0,
        }
    }

    /// Places the scanned entries under `dir`.
    pub fn set_children(&mut self, dir: &Path, entries: Vec<(PathBuf, bool)>) {
        let nodes: Vec<Node> = entries
            .into_iter()
            .map(|(p, is_dir)| Node::new(p, is_dir))
            .collect();
        if dir == self.root {
            self.children = Some(nodes);
        } else if let Some(node) = self.find_node_mut(dir) {
            node.children = Some(nodes);
            node.expanded = true;
        }
    }

    fn find_node_mut(&mut self, target: &Path) -> Option<&mut Node> {
        fn rec<'a>(nodes: &'a mut [Node], target: &Path) -> Option<&'a mut Node> {
            for n in nodes.iter_mut() {
                if n.path == target {
                    return Some(n);
                }
                if let Some(children) = n.children.as_mut()
                    && let Some(found) = rec(children, target) {
                        return Some(found);
                    }
            }
            None
        }
        rec(self.children.as_mut()?, target)
    }

    fn find_node(&self, target: &Path) -> Option<&Node> {
        fn rec<'a>(nodes: &'a [Node], target: &Path) -> Option<&'a Node> {
            for n in nodes.iter() {
                if n.path == target {
                    return Some(n);
                }
                if let Some(children) = n.children.as_ref()
                    && let Some(found) = rec(children, target) {
                        return Some(found);
                    }
            }
            None
        }
        rec(self.children.as_ref()?, target)
    }

    pub fn is_expanded(&self, path: &Path) -> bool {
        self.find_node(path).map(|n| n.expanded).unwrap_or(false)
    }

    pub fn is_loaded(&self, path: &Path) -> bool {
        self.find_node(path)
            .map(|n| n.children.is_some())
            .unwrap_or(false)
    }

    pub fn collapse(&mut self, path: &Path) {
        if let Some(n) = self.find_node_mut(path) {
            n.expanded = false;
        }
    }

    pub fn expand(&mut self, path: &Path) {
        if let Some(n) = self.find_node_mut(path) {
            n.expanded = true;
        }
    }

    /// Flattens the tree into visible rows.
    pub fn visible_rows(&self) -> Vec<VisibleRow> {
        let mut rows = Vec::new();
        fn rec(nodes: &[Node], depth: usize, rows: &mut Vec<VisibleRow>) {
            for n in nodes {
                rows.push(VisibleRow {
                    path: n.path.clone(),
                    name: n.name.clone(),
                    is_dir: n.is_dir,
                    expanded: n.expanded,
                    depth,
                });
                if n.is_dir && n.expanded
                    && let Some(children) = n.children.as_ref() {
                        rec(children, depth + 1, rows);
                    }
            }
        }
        if let Some(children) = self.children.as_ref() {
            rec(children, 0, &mut rows);
        }
        rows
    }
}
