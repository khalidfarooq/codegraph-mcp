use codegraph_core::graph::CodeGraph;
use codegraph_core::integrity::audit_test_diff;
use codegraph_core::shadow::query_with_shadow_overlay;
use codegraph_security::jail::jail_path;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::RwLock;
use tracing::{error, info, warn};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcRequest {
    pub id: u64,
    pub method: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcResponse {
    pub id: u64,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

pub struct DaemonServer {
    pub repo_root: PathBuf,
    pub graph: Arc<RwLock<CodeGraph>>,
    pub untrusted_mode: bool,
}

impl DaemonServer {
    pub fn new(repo_root: PathBuf, untrusted_mode: bool) -> Self {
        Self {
            repo_root,
            graph: Arc::new(RwLock::new(CodeGraph::new(untrusted_mode))),
            untrusted_mode,
        }
    }

    /// Recursively indexes the repository into memory
    pub async fn index_repository(&self) -> usize {
        let mut total_symbols = 0;
        let root = self.repo_root.clone();
        info!("Indexing repository at: {}", root.display());

        let mut files_to_index = Vec::new();
        for entry in WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if entry.file_type().is_file() {
                let path_str = path.to_string_lossy();
                // Skip common noise directories
                if path_str.contains("/.git/")
                    || path_str.contains("/node_modules/")
                    || path_str.contains("/target/")
                    || path_str.contains("/dist/")
                    || path_str.contains("/build/")
                    || path_str.contains("/.turbo/")
                {
                    continue;
                }

                if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                    if matches!(ext, "ts" | "tsx" | "js" | "jsx" | "py" | "rs" | "go") {
                        files_to_index.push(path.to_path_buf());
                    }
                }
            }
        }

        let count = files_to_index.len();
        for file in files_to_index {
            if let Ok(content) = fs::read_to_string(&file) {
                let relative = file.strip_prefix(&root).unwrap_or(&file);
                let mut graph = self.graph.write().await;
                if let Ok(symbols) = graph.add_file(relative, &content) {
                    total_symbols += symbols;
                }
            }
        }

        info!("Indexing complete: {} files, {} symbols", count, total_symbols);
        total_symbols
    }

    /// Dispatches and processes an incoming IPC JSON-RPC request
    pub async fn handle_request(&self, req: IpcRequest) -> IpcResponse {
        match req.method.as_str() {
            "ping" => IpcResponse {
                id: req.id,
                result: Some(serde_json::json!({"status": "pong"})),
                error: None,
            },
            "explore" => {
                let action = req.params.get("action").and_then(|v| v.as_str()).unwrap_or("symbol_context");
                let target = req.params.get("target").and_then(|v| v.as_str()).unwrap_or("");
                let depth = req.params.get("depth").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
                let buffer_content = req.params.get("buffer_content").and_then(|v| v.as_str());

                let graph = self.graph.read().await;

                match action {
                    "symbol_context" => {
                        let path_hint = if target.contains('/') || target.contains('.') {
                            Some(Path::new(target))
                        } else {
                            None
                        };

                        if let Some(ctx) = query_with_shadow_overlay(&graph, target, path_hint, buffer_content) {
                            IpcResponse {
                                id: req.id,
                                result: Some(serde_json::to_value(ctx).unwrap_or_default()),
                                error: None,
                            }
                        } else {
                            IpcResponse {
                                id: req.id,
                                result: None,
                                error: Some(format!("Symbol '{}' not found in codebase graph", target)),
                            }
                        }
                    }
                    "call_hierarchy" => {
                        let direction = req.params.get("direction").and_then(|v| v.as_str()).unwrap_or("incoming");
                        let hierarchy = graph.get_call_hierarchy(target, direction, depth);
                        IpcResponse {
                            id: req.id,
                            result: Some(serde_json::to_value(hierarchy).unwrap_or_default()),
                            error: None,
                        }
                    }
                    "impacted_files" => {
                        let impact = graph.get_impacted_files(target, depth);
                        IpcResponse {
                            id: req.id,
                            result: Some(serde_json::to_value(impact).unwrap_or_default()),
                            error: None,
                        }
                    }
                    _ => IpcResponse {
                        id: req.id,
                        result: None,
                        error: Some(format!("Unknown exploration action: {}", action)),
                    },
                }
            }
            "find_utility" => {
                let intent = req.params.get("intent").and_then(|v| v.as_str()).unwrap_or("");
                let limit = req.params.get("limit").and_then(|v| v.as_u64()).unwrap_or(3) as usize;

                let graph = self.graph.read().await;
                let matches = graph.find_utility(intent, limit);

                IpcResponse {
                    id: req.id,
                    result: Some(serde_json::to_value(matches).unwrap_or_default()),
                    error: None,
                }
            }
            "audit" => {
                let file_path = req.params.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
                let old_content = req.params.get("old_content").and_then(|v| v.as_str()).unwrap_or("");
                let new_content = req.params.get("new_content").and_then(|v| v.as_str()).unwrap_or("");

                let report = audit_test_diff(file_path, old_content, new_content);
                IpcResponse {
                    id: req.id,
                    result: Some(serde_json::to_value(report).unwrap_or_default()),
                    error: None,
                }
            }
            "index_file" => {
                let file_path_str = req.params.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
                let file_path = Path::new(file_path_str);

                if let Ok(safe_path) = jail_path(&self.repo_root, file_path) {
                    if let Ok(content) = fs::read_to_string(&safe_path) {
                        let rel = safe_path.strip_prefix(&self.repo_root).unwrap_or(&safe_path);
                        let mut graph = self.graph.write().await;
                        let _ = graph.add_file(rel, &content);
                    }
                }

                IpcResponse {
                    id: req.id,
                    result: Some(serde_json::json!({"status": "indexed"})),
                    error: None,
                }
            }
            _ => IpcResponse {
                id: req.id,
                result: None,
                error: Some(format!("Unknown method: {}", req.method)),
            },
        }
    }

    /// Starts the Unix Domain Socket listener and serves client requests
    pub async fn run(self: Arc<Self>, socket_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
        // Clean up stale socket file if it exists
        if socket_path.exists() {
            let _ = fs::remove_file(&socket_path);
        }

        let listener = UnixListener::bind(&socket_path)?;
        info!("CodeGraph daemon listening on socket: {}", socket_path.display());

        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let server = Arc::clone(&self);
                    tokio::spawn(async move {
                        if let Err(e) = handle_client(server, stream).await {
                            warn!("Client connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("Socket accept error: {}", e);
                    break;
                }
            }
        }

        Ok(())
    }
}

async fn handle_client(server: Arc<DaemonServer>, stream: UnixStream) -> Result<(), Box<dyn std::error::Error>> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }

        let req: IpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let err_resp = IpcResponse {
                    id: 0,
                    result: None,
                    error: Some(format!("Invalid JSON-RPC frame: {}", e)),
                };
                let resp_str = serde_json::to_string(&err_resp)? + "\n";
                writer.write_all(resp_str.as_bytes()).await?;
                continue;
            }
        };

        let resp = server.handle_request(req).await;
        let mut resp_str = serde_json::to_string(&resp)?;
        resp_str.push('\n');
        writer.write_all(resp_str.as_bytes()).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_daemon_request_handling() {
        let temp_dir = std::env::temp_dir().join("cg_test_daemon_req");
        fs::create_dir_all(&temp_dir).unwrap();
        let test_file = temp_dir.join("utils.ts");
        fs::write(
            &test_file,
            "export function formatCurrency(amount: number): string { return `$${amount}`; }",
        )
        .unwrap();

        let server = DaemonServer::new(temp_dir.clone(), false);
        server.index_repository().await;

        // Test explore
        let req = IpcRequest {
            id: 1,
            method: "explore".to_string(),
            params: serde_json::json!({
                "action": "symbol_context",
                "target": "formatCurrency"
            }),
        };
        let resp = server.handle_request(req).await;
        assert!(resp.result.is_some());
        assert_eq!(resp.id, 1);

        // Test find_utility
        let req_util = IpcRequest {
            id: 2,
            method: "find_utility".to_string(),
            params: serde_json::json!({
                "intent": "format currency with dollar"
            }),
        };
        let resp_util = server.handle_request(req_util).await;
        assert!(resp_util.result.is_some());

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
