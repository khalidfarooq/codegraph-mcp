use codegraph_security::SanitizedEnvelope;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Language {
    TypeScript,
    JavaScript,
    Python,
    Rust,
    Go,
    Unknown,
}

impl Language {
    pub fn from_extension(ext: &str) -> Self {
        match ext {
            "ts" | "tsx" | "mts" | "cts" => Language::TypeScript,
            "js" | "jsx" | "mjs" | "cjs" => Language::JavaScript,
            "py" | "pyi" => Language::Python,
            "rs" => Language::Rust,
            "go" => Language::Go,
            _ => Language::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Interface,
    TypeAlias,
    Struct,
    Enum,
    Variable,
    Constant,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceRange {
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
    pub byte_offset: usize,
    pub byte_length: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolDef {
    pub name: String,
    pub file_path: PathBuf,
    pub language: Language,
    pub kind: SymbolKind,
    pub signature: String,
    pub docstring: Option<SanitizedEnvelope>,
    pub is_exported: bool,
    pub range: SourceRange,
    pub score: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallReference {
    pub caller_symbol: String,
    pub callee_symbol: String,
    pub file_path: PathBuf,
    pub line_number: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallerInfo {
    pub name: String,
    pub location: String,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolContext {
    pub symbol: String,
    pub defined_at: String,
    pub kind: SymbolKind,
    pub signature: String,
    pub docstring: Option<SanitizedEnvelope>,
    pub confidence: String,
    pub callers: Vec<CallerInfo>,
    pub callees: Vec<String>,
    pub downstream_dependencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallHierarchyNode {
    pub name: String,
    pub location: String,
    pub calls: Vec<CallHierarchyNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallHierarchy {
    pub root_symbol: String,
    pub direction: String,
    pub nodes: Vec<CallHierarchyNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AffectedFile {
    pub path: String,
    pub reason: String,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactedFiles {
    pub target: String,
    pub action: String,
    pub blast_radius_score: usize,
    pub affected_files: Vec<AffectedFile>,
    pub downstream_tests: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtilityMatch {
    pub name: String,
    pub file_path: String,
    pub line_number: usize,
    pub signature: String,
    pub docstring: Option<SanitizedEnvelope>,
    pub similarity_score: f32,
    pub repo_usage_count: usize,
    pub is_exported: bool,
}
