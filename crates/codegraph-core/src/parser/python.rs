use super::ExtractedFile;
use crate::types::{CallReference, Language, SourceRange, SymbolDef, SymbolKind};
use codegraph_security::sanitizer::sanitize_text;
use std::path::Path;
use tree_sitter::{Node, Parser};

pub fn parse_python(
    file_path: &Path,
    content: &str,
    untrusted_mode: bool,
) -> Result<ExtractedFile, super::ParseError> {
    let mut parser = Parser::new();
    let lang = tree_sitter_python::language();
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
) {
    match node.kind() {
        "import_statement" | "import_from_statement" => {
            if let Ok(text) = node.utf8_text(bytes) {
                imports.push(text.to_string());
            }
        }
        "function_definition" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                if let Ok(name) = name_node.utf8_text(bytes) {
                    let doc = extract_python_docstring(node, bytes, untrusted_mode);
                    let sig = extract_signature(node, bytes);
                    let range = make_range(node);
                    let is_exported = !name.starts_with('_');
                    symbols.push(SymbolDef {
                        name: name.to_string(),
                        file_path: file_path.to_path_buf(),
                        language: Language::Python,
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
        "class_definition" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                if let Ok(name) = name_node.utf8_text(bytes) {
                    let doc = extract_python_docstring(node, bytes, untrusted_mode);
                    let sig = format!("class {}", name);
                    let range = make_range(node);
                    let is_exported = !name.starts_with('_');
                    symbols.push(SymbolDef {
                        name: name.to_string(),
                        file_path: file_path.to_path_buf(),
                        language: Language::Python,
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
        "call" => {
            if let Some(func_node) = node.child_by_field_name("function") {
                if let Ok(callee) = func_node.utf8_text(bytes) {
                    let clean_callee = if let Some((_, last)) = callee.rsplit_once('.') {
                        last.trim()
                    } else {
                        callee.trim()
                    };
                    calls.push(CallReference {
                        caller_symbol: "".to_string(),
                        callee_symbol: clean_callee.to_string(),
                        file_path: file_path.to_path_buf(),
                        line_number: node.start_position().row + 1,
                    });
                }
            }
        }
        _ => {}
    }

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
        );
    }
}

fn extract_signature(node: Node, bytes: &[u8]) -> String {
    if let Ok(text) = node.utf8_text(bytes) {
        if let Some(idx) = text.find(':') {
            return text[..idx].trim().to_string();
        }
        text.lines().next().unwrap_or(text).trim().to_string()
    } else {
        "".to_string()
    }
}

fn extract_python_docstring(
    node: Node,
    bytes: &[u8],
    untrusted_mode: bool,
) -> Option<codegraph_security::SanitizedEnvelope> {
    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            if child.kind() == "expression_statement" {
                if let Some(str_node) = child.child(0) {
                    if str_node.kind() == "string" {
                        if let Ok(text) = str_node.utf8_text(bytes) {
                            let clean = text.trim_matches('"').trim_matches('\'').trim();
                            return Some(sanitize_text(clean, untrusted_mode));
                        }
                    }
                }
            }
            break;
        }
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
