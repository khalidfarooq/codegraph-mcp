pub mod go_lang;
pub mod python;
pub mod rust_lang;
pub mod typescript;

use crate::types::{CallReference, Language, SymbolDef};
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("Unsupported language for file: {0}")]
    UnsupportedLanguage(String),
    #[error("Failed to parse file {0}: {1}")]
    TreeSitterError(String, String),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

#[derive(Debug, Clone, Default)]
pub struct ExtractedFile {
    pub symbols: Vec<SymbolDef>,
    pub calls: Vec<CallReference>,
    pub imports: Vec<String>,
}

/// Parses source code and extracts symbols, function calls, and import statements
pub fn parse_file(
    file_path: &Path,
    content: &str,
    untrusted_mode: bool,
) -> Result<ExtractedFile, ParseError> {
    let ext = file_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let lang = Language::from_extension(ext);

    match lang {
        Language::TypeScript | Language::JavaScript => {
            let is_tsx = ext == "tsx" || ext == "jsx";
            typescript::parse_typescript(file_path, content, is_tsx, untrusted_mode)
        }
        Language::Python => python::parse_python(file_path, content, untrusted_mode),
        Language::Rust => rust_lang::parse_rust(file_path, content, untrusted_mode),
        Language::Go => go_lang::parse_go(file_path, content, untrusted_mode),
        Language::Unknown => Err(ParseError::UnsupportedLanguage(file_path.display().to_string())),
    }
}
