use crate::graph::CodeGraph;
use crate::parser::parse_file;
use crate::types::SymbolContext;
use std::path::Path;

/// Evaluates a symbol query against an unsaved in-memory buffer before falling back to the persistent graph
pub fn query_with_shadow_overlay(
    graph: &CodeGraph,
    symbol: &str,
    file_path: Option<&Path>,
    buffer_content: Option<&str>,
) -> Option<SymbolContext> {
    // 1. If buffer_content is provided, parse ephemeral shadow nodes
    if let (Some(buf), Some(path)) = (buffer_content, file_path) {
        if let Ok(extracted) = parse_file(path, buf, graph.untrusted_mode) {
            if let Some(target_sym) = extracted.symbols.iter().find(|s| s.name == symbol) {
                // Return context directly from dirty buffer with callers from persistent graph
                let callers = if let Some(persistent_ctx) = graph.get_symbol_context(symbol, Some(path)) {
                    persistent_ctx.callers
                } else {
                    Vec::new()
                };

                return Some(SymbolContext {
                    symbol: target_sym.name.clone(),
                    defined_at: format!("{}:{} (unsaved buffer)", path.display(), target_sym.range.start_line),
                    kind: target_sym.kind,
                    signature: target_sym.signature.clone(),
                    docstring: target_sym.docstring.clone(),
                    confidence: "shadow_overlay".to_string(),
                    callers,
                    callees: Vec::new(),
                    downstream_dependencies: Vec::new(),
                });
            }
        }
    }

    // 2. Fall back to persistent graph
    graph.get_symbol_context(symbol, file_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shadow_overlay() {
        let graph = CodeGraph::new(false);
        let dirty_ts = "export function dirtyNewHelper(x: string): boolean { return true; }";

        let ctx = query_with_shadow_overlay(
            &graph,
            "dirtyNewHelper",
            Some(Path::new("src/temp.ts")),
            Some(dirty_ts),
        )
        .unwrap();

        assert_eq!(ctx.symbol, "dirtyNewHelper");
        assert_eq!(ctx.confidence, "shadow_overlay");
    }
}
