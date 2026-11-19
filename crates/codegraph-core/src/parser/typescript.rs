use super::ExtractedFile;
use crate::types::{CallReference, Language, SourceRange, SymbolDef, SymbolKind};
use codegraph_security::sanitizer::sanitize_text;
use std::path::Path;
use tree_sitter::{Node, Parser};

pub fn parse_typescript(
    file_path: &Path,
    content: &str,
    is_tsx: bool,
    untrusted_mode: bool,
) -> Result<ExtractedFile, super::ParseError> {
    let mut parser = Parser::new();
    let lang = if is_tsx {
        tree_sitter_typescript::language_tsx()
    } else {
        tree_sitter_typescript::language_typescript()
    };
    parser
        .set_language(&lang)
        .map_err(|e| super::ParseError::TreeSitterError(file_path.display().to_string(), e.to_string()))?;

    let tree = parser.parse(content, None).ok_or_else(|| {
        super::ParseError::TreeSitterError(file_path.display().to_string(), "Parse failed".to_string())
    })?;

    let mut symbols = Vec::new();
    let mut calls = Vec::new();
    let mut imports = Vec::new();

    let root = tree.root_node();
    let content_bytes = content.as_bytes();

    walk_node(
        root,
        content_bytes,
        file_path,
        &mut symbols,
        &mut calls,
        &mut imports,
        untrusted_mode,
        false,
    );

    Ok(ExtractedFile {
        symbols,
        calls,
        imports,
    })
}

fn walk_node(
    node: Node,
    bytes: &[u8],
    file_path: &Path,
    symbols: &mut Vec<SymbolDef>,
    calls: &mut Vec<CallReference>,
    imports: &mut Vec<String>,
    untrusted_mode: bool,
    is_exported: bool,
) {
    match node.kind() {
        "export_statement" => {
            // Children of export_statement are exported
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() != "export" && child.kind() != "default" {
                    walk_node(child, bytes, file_path, symbols, calls, imports, untrusted_mode, true);
                }
            }
            return;
        }
        "import_statement" => {
            if let Ok(text) = node.utf8_text(bytes) {
                imports.push(text.to_string());
            }
        }
        "function_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                if let Ok(name) = name_node.utf8_text(bytes) {
                    let doc = extract_preceding_doc(node, bytes, untrusted_mode);
                    let sig = extract_signature(node, bytes);
                    let range = make_range(node);
                    symbols.push(SymbolDef {
                        name: name.to_string(),
                        file_path: file_path.to_path_buf(),
                        language: Language::TypeScript,
                        kind: SymbolKind::Function,
                        signature: sig,
                        docstring: doc,
                        is_exported,
                        range,
                        score: None,
                    });
                }
            }
        }
        "class_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                if let Ok(name) = name_node.utf8_text(bytes) {
                    let doc = extract_preceding_doc(node, bytes, untrusted_mode);
                    let sig = format!("class {}", name);
                    let range = make_range(node);
                    symbols.push(SymbolDef {
                        name: name.to_string(),
                        file_path: file_path.to_path_buf(),
                        language: Language::TypeScript,
                        kind: SymbolKind::Class,
                        signature: sig,
                        docstring: doc,
                        is_exported,
                        range,
                        score: None,
                    });
                }
            }
        }
        "interface_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                if let Ok(name) = name_node.utf8_text(bytes) {
                    let doc = extract_preceding_doc(node, bytes, untrusted_mode);
                    let sig = format!("interface {}", name);
                    let range = make_range(node);
                    symbols.push(SymbolDef {
                        name: name.to_string(),
                        file_path: file_path.to_path_buf(),
                        language: Language::TypeScript,
                        kind: SymbolKind::Interface,
                        signature: sig,
                        docstring: doc,
                        is_exported,
                        range,
                        score: None,
                    });
                }
            }
        }
        "type_alias_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                if let Ok(name) = name_node.utf8_text(bytes) {
                    let doc = extract_preceding_doc(node, bytes, untrusted_mode);
                    let sig = extract_signature(node, bytes);
                    let range = make_range(node);
                    symbols.push(SymbolDef {
                        name: name.to_string(),
                        file_path: file_path.to_path_buf(),
                        language: Language::TypeScript,
                        kind: SymbolKind::TypeAlias,
                        signature: sig,
                        docstring: doc,
                        is_exported,
                        range,
                        score: None,
                    });
                }
            }
        }
        "method_definition" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                if let Ok(name) = name_node.utf8_text(bytes) {
                    let doc = extract_preceding_doc(node, bytes, untrusted_mode);
                    let sig = extract_signature(node, bytes);
                    let range = make_range(node);
                    symbols.push(SymbolDef {
                        name: name.to_string(),
                        file_path: file_path.to_path_buf(),
                        language: Language::TypeScript,
                        kind: SymbolKind::Method,
                        signature: sig,
                        docstring: doc,
                        is_exported,
                        range,
                        score: None,
                    });
                }
            }
        }
        "call_expression" => {
            if let Some(func_node) = node.child_by_field_name("function") {
                if let Ok(callee) = func_node.utf8_text(bytes) {
                    // Extract simple function or method call name
                    let clean_callee = if let Some((_, last)) = callee.rsplit_once('.') {
                        last.trim()
                    } else {
                        callee.trim()
                    };
                    calls.push(CallReference {
                        caller_symbol: "".to_string(), // Resolved during graph pass
                        callee_symbol: clean_callee.to_string(),
                        file_path: file_path.to_path_buf(),
                        line_number: node.start_position().row + 1,
                    });
                }
            }
        }
        _ => {}
    }

    // Traverse children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_node(
            child,
            bytes,
            file_path,
            symbols,
            calls,
            imports,
            untrusted_mode,
            is_exported,
        );
    }
}

fn extract_signature(node: Node, bytes: &[u8]) -> String {
    if let Ok(text) = node.utf8_text(bytes) {
        if let Some(idx) = text.find('{') {
            return text[..idx].trim().to_string();
        } else if let Some(idx) = text.find(';') {
            return text[..idx].trim().to_string();
        }
        text.lines().next().unwrap_or(text).trim().to_string()
    } else {
        "".to_string()
    }
}

fn extract_preceding_doc(node: Node, bytes: &[u8], untrusted_mode: bool) -> Option<codegraph_security::SanitizedEnvelope> {
    let mut prev = node.prev_sibling();
    while let Some(sibling) = prev {
        if sibling.kind() == "comment" {
            if let Ok(comment_text) = sibling.utf8_text(bytes) {
                if comment_text.starts_with("/**") || comment_text.starts_with("//") {
                    return Some(sanitize_text(comment_text, untrusted_mode));
                }
            }
        } else if !sibling.kind().is_empty() && sibling.kind() != "export" {
            break;
        }
        prev = sibling.prev_sibling();
    }
    None
}

fn make_range(node: Node) -> SourceRange {
    SourceRange {
        start_line: node.start_position().row + 1,
        start_col: node.start_position().column,
        end_line: node.end_position().row + 1,
        end_col: node.end_position().column,
        byte_offset: node.start_byte(),
        byte_length: node.end_byte().saturating_sub(node.start_byte()),
    }
}
