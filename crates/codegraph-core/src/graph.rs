use crate::bm25::BM25Index;
use crate::parser::parse_file;
use crate::types::*;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub enum EdgeType {
    Calls,
    Imports,
}

pub struct CodeGraph {
    pub graph: DiGraph<SymbolDef, EdgeType>,
    pub name_to_nodes: HashMap<String, Vec<NodeIndex>>,
    pub file_to_nodes: HashMap<PathBuf, Vec<NodeIndex>>,
    pub bm25: BM25Index,
    pub untrusted_mode: bool,
    pub next_doc_id: usize,
    pub node_to_doc_id: HashMap<NodeIndex, usize>,
    pub doc_id_to_node: HashMap<usize, NodeIndex>,
}

impl CodeGraph {
    pub fn new(untrusted_mode: bool) -> Self {
        Self {
            graph: DiGraph::new(),
            name_to_nodes: HashMap::new(),
            file_to_nodes: HashMap::new(),
            bm25: BM25Index::new(),
            untrusted_mode,
            next_doc_id: 0,
            node_to_doc_id: HashMap::new(),
            doc_id_to_node: HashMap::new(),
        }
    }

    /// Indexes a file's AST and connects call edges into the persistent graph
    pub fn add_file(&mut self, path: &Path, content: &str) -> Result<usize, crate::parser::ParseError> {
        self.remove_file(path);

        let extracted = parse_file(path, content, self.untrusted_mode)?;
        let count = extracted.symbols.len();

        let mut current_file_nodes = Vec::new();

        // 1. Add symbol nodes
        for sym in extracted.symbols {
            let name = sym.name.clone();
            let text_to_index = format!(
                "{} {} {}",
                sym.name,
                sym.signature,
                sym.docstring.as_ref().map(|d| d.content.as_str()).unwrap_or("")
            );

            let node_idx = self.graph.add_node(sym);
            self.name_to_nodes
                .entry(name)
                .or_default()
                .push(node_idx);

            current_file_nodes.push(node_idx);

            // Add to BM25 index
            let doc_id = self.next_doc_id;
            self.next_doc_id += 1;
            self.bm25.add_document(doc_id, &text_to_index);
            self.node_to_doc_id.insert(node_idx, doc_id);
            self.doc_id_to_node.insert(doc_id, node_idx);
        }

        // 2. Connect intra-file and inter-file call edges
        for call in extracted.calls {
            if let Some(target_nodes) = self.name_to_nodes.get(&call.callee_symbol) {
                for &target_node in target_nodes {
                    // Find caller in current file enclosing the line
                    if let Some(&caller_node) = current_file_nodes.iter().find(|&&n| {
                        let range = &self.graph[n].range;
                        call.line_number >= range.start_line && call.line_number <= range.end_line
                    }) {
                        self.graph.add_edge(caller_node, target_node, EdgeType::Calls);
                    }
                }
            }
        }

        self.file_to_nodes.insert(path.to_path_buf(), current_file_nodes);
        Ok(count)
    }

    /// Removes all symbols and edges associated with a file
    pub fn remove_file(&mut self, path: &Path) {
        if let Some(nodes) = self.file_to_nodes.remove(path) {
            // Sort in reverse order to keep petgraph node indices stable during removal
            let mut sorted = nodes.clone();
            sorted.sort_by(|a, b| b.cmp(a));

            for node in sorted {
                let name = self.graph[node].name.clone();
                if let Some(indices) = self.name_to_nodes.get_mut(&name) {
                    indices.retain(|&idx| idx != node);
                    if indices.is_empty() {
                        self.name_to_nodes.remove(&name);
                    }
                }
                if let Some(doc_id) = self.node_to_doc_id.remove(&node) {
                    self.doc_id_to_node.remove(&doc_id);
                }
                self.graph.remove_node(node);
            }
        }
    }

    /// Resolves detailed symbol context (definition, signature, callers, callees)
    pub fn get_symbol_context(&self, symbol: &str, file_hint: Option<&Path>) -> Option<SymbolContext> {
        let node_indices = self.name_to_nodes.get(symbol)?;
        if node_indices.is_empty() {
            return None;
        }

        // Disambiguate by file_hint if provided
        let target_node = if let Some(hint) = file_hint {
            node_indices
                .iter()
                .copied()
                .find(|&n| self.graph[n].file_path.ends_with(hint))
                .unwrap_or(node_indices[0])
        } else {
            node_indices[0]
        };

        let sym = &self.graph[target_node];

        // Collect incoming callers (who calls this symbol?)
        let mut callers = Vec::new();
        for edge in self.graph.edges_directed(target_node, Direction::Incoming) {
            let caller_sym = &self.graph[edge.source()];
            callers.push(CallerInfo {
                name: caller_sym.name.clone(),
                location: caller_sym.file_path.display().to_string(),
                line: caller_sym.range.start_line,
            });
        }

        // Collect outgoing callees (who does this symbol call?)
        let mut callees = Vec::new();
        for edge in self.graph.edges_directed(target_node, Direction::Outgoing) {
            let callee_sym = &self.graph[edge.target()];
            callees.push(callee_sym.name.clone());
        }

        // Downstream dependencies
        let mut downstream = HashSet::new();
        for edge in self.graph.edges_directed(target_node, Direction::Incoming) {
            downstream.insert(self.graph[edge.source()].name.clone());
        }

        Some(SymbolContext {
            symbol: sym.name.clone(),
            defined_at: format!("{}:{}", sym.file_path.display(), sym.range.start_line),
            kind: sym.kind,
            signature: sym.signature.clone(),
            docstring: sym.docstring.clone(),
            confidence: "high".to_string(),
            callers,
            callees,
            downstream_dependencies: downstream.into_iter().collect(),
        })
    }

    /// Explores recursive call hierarchy
    pub fn get_call_hierarchy(&self, symbol: &str, direction: &str, depth: usize) -> CallHierarchy {
        let mut root_nodes = Vec::new();
        if let Some(indices) = self.name_to_nodes.get(symbol) {
            for &idx in indices {
                let dir = if direction == "incoming" || direction == "callers" {
                    Direction::Incoming
                } else {
                    Direction::Outgoing
                };
                let node = self.build_hierarchy_node(idx, dir, depth, 0);
                root_nodes.push(node);
            }
        }

        CallHierarchy {
            root_symbol: symbol.to_string(),
            direction: direction.to_string(),
            nodes: root_nodes,
        }
    }

    fn build_hierarchy_node(
        &self,
        node_idx: NodeIndex,
        dir: Direction,
        max_depth: usize,
        current_depth: usize,
    ) -> CallHierarchyNode {
        let sym = &self.graph[node_idx];
        let mut child_nodes = Vec::new();

        if current_depth < max_depth {
            for edge in self.graph.edges_directed(node_idx, dir) {
                let neighbor = match dir {
                    Direction::Incoming => edge.source(),
                    Direction::Outgoing => edge.target(),
                };
                child_nodes.push(self.build_hierarchy_node(neighbor, dir, max_depth, current_depth + 1));
            }
        }

        CallHierarchyNode {
            name: sym.name.clone(),
            location: format!("{}:{}", sym.file_path.display(), sym.range.start_line),
            calls: child_nodes,
        }
    }

    /// Computes blast radius impact analysis across the monorepo
    pub fn get_impacted_files(&self, target: &str, depth: usize) -> ImpactedFiles {
        let mut affected = Vec::new();
        let mut downstream_tests = Vec::new();
        let mut visited = HashSet::new();

        if let Some(indices) = self.name_to_nodes.get(target) {
            for &root in indices {
                self.collect_impact(root, depth, 0, &mut affected, &mut downstream_tests, &mut visited);
            }
        }

        let blast_radius = affected.len();
        ImpactedFiles {
            target: target.to_string(),
            action: "impacted_files".to_string(),
            blast_radius_score: blast_radius,
            affected_files: affected,
            downstream_tests,
        }
    }

    fn collect_impact(
        &self,
        node: NodeIndex,
        max_depth: usize,
        current_depth: usize,
        affected: &mut Vec<AffectedFile>,
        tests: &mut Vec<String>,
        visited: &mut HashSet<NodeIndex>,
    ) {
        if !visited.insert(node) || current_depth >= max_depth {
            return;
        }

        for edge in self.graph.edges_directed(node, Direction::Incoming) {
            let caller_idx = edge.source();
            let caller = &self.graph[caller_idx];
            let file_str = caller.file_path.display().to_string();

            if is_test_file(&file_str) {
                if !tests.contains(&file_str) {
                    tests.push(file_str.clone());
                }
            } else {
                affected.push(AffectedFile {
                    path: file_str,
                    reason: format!("calls '{}'", self.graph[node].name),
                    line: caller.range.start_line,
                });
            }

            self.collect_impact(caller_idx, max_depth, current_depth + 1, affected, tests, visited);
        }
    }

    /// Semantic and lexical utility lookup to prevent redundant code generation
    pub fn find_utility(&self, intent: &str, limit: usize) -> Vec<UtilityMatch> {
        let ranked = self.bm25.query(intent, limit);
        let mut matches = Vec::new();

        for (doc_id, score) in ranked {
            if let Some(&node_idx) = self.doc_id_to_node.get(&doc_id) {
                let sym = &self.graph[node_idx];
                let usage_count = self
                    .graph
                    .edges_directed(node_idx, Direction::Incoming)
                    .count();

                matches.push(UtilityMatch {
                    name: sym.name.clone(),
                    file_path: sym.file_path.display().to_string(),
                    line_number: sym.range.start_line,
                    signature: sym.signature.clone(),
                    docstring: sym.docstring.clone(),
                    similarity_score: (score / (score + 10.0)).clamp(0.0, 1.0),
                    repo_usage_count: usage_count,
                    is_exported: sym.is_exported,
                });
            }
        }

        matches
    }
}

fn is_test_file(path: &str) -> bool {
    path.contains("test")
        || path.contains("spec")
        || path.ends_with("_test.go")
        || path.ends_with("_test.py")
        || path.ends_with(".test.ts")
        || path.ends_with(".spec.ts")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_add_and_query() {
        let mut cg = CodeGraph::new(false);
        let ts_code = r#"
            export function formatMoney(amount: number): string {
                return `$${amount.toFixed(2)}`;
            }

            export function checkout() {
                const total = formatMoney(42);
                console.log(total);
            }
        "#;

        let count = cg.add_file(Path::new("src/checkout.ts"), ts_code).unwrap();
        assert_eq!(count, 2);

        let ctx = cg.get_symbol_context("formatMoney", None).unwrap();
        assert_eq!(ctx.symbol, "formatMoney");
        assert_eq!(ctx.callers.len(), 1);
        assert_eq!(ctx.callers[0].name, "checkout");

        let utils = cg.find_utility("money currency format", 1);
        assert!(!utils.is_empty());
        assert_eq!(utils[0].name, "formatMoney");
    }
}
