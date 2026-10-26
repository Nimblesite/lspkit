//! V8 `.heapprofile` export. Implements [PROF-MEMORY-HEAPPROFILE].
//!
//! The Chrome `DevTools` `SamplingHeapProfile` schema, so the editor's
//! built-in viewer renders a snapshot natively. Each site's traceback becomes
//! a root-to-leaf path and shared prefixes merge into a real call tree;
//! `selfSize` accumulates at each path's leaf. A located frame is labelled
//! with its source line (each file read once), a type-keyed site with its
//! type name.

use std::collections::HashMap;
use std::path::Path;

use serde_json::{json, Value};

use crate::wire::{AllocationSite, HeapSnapshot};

/// Largest source file read to label a frame; bigger files fall back to the
/// file name so a generated file never stalls the export.
const MAX_LABEL_SOURCE_BYTES: u64 = 2 * 1024 * 1024;

/// One node of the tree under construction.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
enum NodeKey {
    /// A source location.
    Located(String, i64),
    /// A type with no location.
    Type(String),
}

/// Arena node accumulating a merged allocation path for V8's heap viewer.
struct TreeNode {
    /// Source or type identity; `None` marks the synthetic root.
    key: Option<NodeKey>,
    /// Bytes allocated at this leaf rather than in its descendants.
    self_size: i64,
    /// Stable identifier referenced by the profile's allocation samples.
    id: u64,
    /// Arena indices of child nodes in insertion order.
    children: Vec<usize>,
    /// Reuses children shared by allocation paths with the same prefix.
    index: HashMap<NodeKey, usize>,
}

impl TreeNode {
    /// Create a node with no allocations or merged children yet.
    fn new(id: u64, key: Option<NodeKey>) -> Self {
        Self {
            key,
            self_size: 0,
            id,
            children: Vec::new(),
            index: HashMap::new(),
        }
    }
}

/// Build the `.heapprofile` value for a snapshot.
#[must_use]
pub fn build(snapshot: &HeapSnapshot) -> Value {
    let mut nodes = vec![TreeNode::new(0, None)];
    let samples: Vec<Value> = snapshot
        .sites
        .iter()
        .enumerate()
        .filter_map(|(ordinal, site)| {
            let leaf = insert_path(&mut nodes, site);
            let node = nodes.get_mut(leaf)?;
            node.self_size += site.size_bytes;
            Some(json!({ "size": site.size_bytes, "nodeId": node.id, "ordinal": ordinal }))
        })
        .collect();
    let mut labels = SourceLabels::default();
    let head = nodes
        .first()
        .map_or_else(|| json!({}), |root| node_json(&nodes, root, &mut labels));
    json!({ "head": head, "samples": samples })
}

/// Write the `.heapprofile` for a snapshot into `output_dir`.
///
/// # Errors
///
/// Returns a message when serialisation or the write fails.
pub fn export(snapshot: &HeapSnapshot, output_dir: &Path) -> Result<std::path::PathBuf, String> {
    let json = serde_json::to_string(&build(snapshot))
        .map_err(|err| format!("Failed to serialize heapprofile: {err}"))?;
    let path = output_dir.join(format!("lsp-profiler-{}.heapprofile", snapshot.snapshot_id));
    crate::export::write_artifact(&path, json.as_bytes())
}

/// Insert a site's path; returns the leaf's arena index.
fn insert_path(nodes: &mut Vec<TreeNode>, site: &AllocationSite) -> usize {
    path_of(site).into_iter().fold(0, |cursor, key| {
        if let Some(child) = nodes
            .get(cursor)
            .and_then(|node| node.index.get(&key).copied())
        {
            return child;
        }
        let child = nodes.len();
        let id = u64::try_from(child).unwrap_or(u64::MAX);
        nodes.push(TreeNode::new(id, Some(key.clone())));
        if let Some(parent) = nodes.get_mut(cursor) {
            let _ = parent.index.insert(key, child);
            parent.children.push(child);
        }
        child
    })
}

/// Root-to-leaf path of a site: its traceback, else its location, else its type.
fn path_of(site: &AllocationSite) -> Vec<NodeKey> {
    if !site.traceback.is_empty() {
        return site
            .traceback
            .iter()
            .map(|f| NodeKey::Located(f.file.clone(), f.line))
            .collect();
    }
    match (&site.file, site.line, &site.type_name) {
        (Some(file), Some(line), _) => vec![NodeKey::Located(file.clone(), line)],
        (_, _, Some(type_name)) => vec![NodeKey::Type(type_name.clone())],
        _ => vec![NodeKey::Type("(unknown)".to_owned())],
    }
}

/// Recursively serialize a merged allocation subtree in V8's schema.
fn node_json(nodes: &[TreeNode], node: &TreeNode, labels: &mut SourceLabels) -> Value {
    let children: Vec<Value> = node
        .children
        .iter()
        .filter_map(|child| nodes.get(*child))
        .map(|child| node_json(nodes, child, labels))
        .collect();
    json!({ "callFrame": call_frame(node.key.as_ref(), labels), "selfSize": node.self_size, "id": node.id, "children": children })
}

/// V8 call frame; lines are 0-based.
fn call_frame(key: Option<&NodeKey>, labels: &mut SourceLabels) -> Value {
    let (name, url, line) = match key {
        None => ("(root)".to_owned(), String::new(), -1),
        Some(NodeKey::Type(type_name)) => (type_name.clone(), String::new(), -1),
        Some(NodeKey::Located(file, line)) => {
            (labels.label(file, *line), file.clone(), (line - 1).max(0))
        }
    };
    json!({ "functionName": name, "scriptId": "0", "url": url, "lineNumber": line, "columnNumber": if line < 0 { -1 } else { 0 } })
}

/// Source-line labels, reading each file at most once.
#[derive(Default)]
struct SourceLabels {
    /// Source text by path, including failed reads to avoid repeated I/O.
    cache: HashMap<String, Option<Vec<String>>>,
}

impl SourceLabels {
    /// Prefer the referenced source text, falling back to the file basename.
    fn label(&mut self, file: &str, line: i64) -> String {
        let lines = self
            .cache
            .entry(file.to_owned())
            .or_insert_with(|| read_lines(file));
        usize::try_from(line - 1)
            .ok()
            .and_then(|index| lines.as_ref().and_then(|all| all.get(index)))
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| basename(file))
    }
}

/// Load a bounded source file for labels, or decline unreadable/oversized input.
fn read_lines(file: &str) -> Option<Vec<String>> {
    if std::fs::metadata(file).ok()?.len() > MAX_LABEL_SOURCE_BYTES {
        return None;
    }
    std::fs::read_to_string(file)
        .ok()
        .map(|text| text.lines().map(str::to_owned).collect())
}

/// Produce a useful location label without exposing the full source text.
fn basename(file: &str) -> String {
    Path::new(file).file_name().map_or_else(
        || file.to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::TraceFrame;

    fn site(file: &str, line: i64, size: i64, traceback: Vec<(&str, i64)>) -> AllocationSite {
        AllocationSite {
            file: Some(file.to_owned()),
            line: Some(line),
            type_name: None,
            size_bytes: size,
            count: 1,
            traceback: traceback
                .into_iter()
                .map(|(f, l)| TraceFrame {
                    file: f.to_owned(),
                    line: l,
                })
                .collect(),
        }
    }

    fn snapshot(sites: Vec<AllocationSite>) -> HeapSnapshot {
        HeapSnapshot {
            snapshot_id: "snap-1".to_owned(),
            current_bytes: 0,
            peak_bytes: 0,
            object_count: 0,
            gc_generations: vec![],
            generation_sizes: None,
            sites,
        }
    }

    // [PROF-MEMORY-HEAPPROFILE] head tree + samples, selfSize at the leaf,
    // 0-based lines, basename label when the source is unreadable.
    #[test]
    fn heapprofile_matches_v8_schema() {
        let profile = build(&snapshot(vec![site(
            "/nonexistent/app.py",
            42,
            24_567_890,
            vec![("/nonexistent/app.py", 42)],
        )]));
        assert_eq!(
            profile.pointer("/head/selfSize").and_then(Value::as_i64),
            Some(0)
        );
        assert_eq!(
            profile
                .pointer("/head/children/0/selfSize")
                .and_then(Value::as_i64),
            Some(24_567_890)
        );
        assert_eq!(
            profile
                .pointer("/head/children/0/callFrame/lineNumber")
                .and_then(Value::as_i64),
            Some(41)
        );
        assert_eq!(
            profile
                .pointer("/head/children/0/callFrame/functionName")
                .and_then(Value::as_str),
            Some("app.py")
        );
        assert_eq!(
            profile.pointer("/samples/0/size").and_then(Value::as_i64),
            Some(24_567_890)
        );
    }

    // Shared traceback prefixes merge into one subtree.
    #[test]
    fn shared_prefixes_merge_into_a_call_tree() {
        let a = site(
            "/x/leaf_a.py",
            9,
            100,
            vec![("/x/main.py", 1), ("/x/leaf_a.py", 9)],
        );
        let b = site(
            "/x/leaf_b.py",
            7,
            50,
            vec![("/x/main.py", 1), ("/x/leaf_b.py", 7)],
        );
        let profile = build(&snapshot(vec![a, b]));
        assert_eq!(
            profile
                .pointer("/head/children")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(1),
            "one shared root frame"
        );
        assert_eq!(
            profile
                .pointer("/head/children/0/children")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(2)
        );
    }

    // Labels come from the source line when the file is readable.
    #[test]
    fn labels_use_the_source_line() {
        let dir = tempfile::tempdir().expect("create temporary heapprofile directory");
        let file = dir.path().join("cache.py");
        std::fs::write(&file, "x = 1\n   cache.append(row)   \n")
            .expect("write source used for heap labels");
        let path = file.display().to_string();
        let profile = build(&snapshot(vec![site(&path, 2, 10, vec![])]));
        assert_eq!(
            profile
                .pointer("/head/children/0/callFrame/functionName")
                .and_then(Value::as_str),
            Some("cache.append(row)")
        );
    }

    // Type-keyed sites (no location) become type-named nodes.
    #[test]
    fn type_sites_become_type_nodes() {
        let by_type = AllocationSite {
            file: None,
            line: None,
            type_name: Some("System.String".to_owned()),
            size_bytes: 64,
            count: 2,
            traceback: vec![],
        };
        let profile = build(&snapshot(vec![by_type]));
        assert_eq!(
            profile
                .pointer("/head/children/0/callFrame/functionName")
                .and_then(Value::as_str),
            Some("System.String")
        );
        assert_eq!(
            profile
                .pointer("/head/children/0/callFrame/lineNumber")
                .and_then(Value::as_i64),
            Some(-1)
        );
    }

    #[test]
    fn empty_snapshot_has_no_children() {
        let profile = build(&snapshot(vec![]));
        assert_eq!(
            profile
                .pointer("/head/children")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(0)
        );
    }
}
