use crate::client::DaemonClient;
use codegraph_core::graph::CodeGraph;
use codegraph_core::shadow::query_with_shadow_overlay;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::RwLock;
use tracing::info;
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: Option<String>,
    pub id: Option<serde_json::Value>,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<serde_json::Value>,
}

pub struct McpServer {
    pub repo_root: PathBuf,
    pub client: DaemonClient,
    pub fallback_graph: Arc<RwLock<Option<CodeGraph>>>,
}

impl McpServer {
    pub fn new(repo_root: PathBuf) -> Result<Self, std::io::Error> {
        let client = DaemonClient::for_repo(&repo_root)?;
        Ok(Self {
            repo_root,
            client,
            fallback_graph: Arc::new(RwLock::new(None)),
        })
    }

    /// Handles standard MCP tool calls
    pub async fn handle_tool_call(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        // 1. Try sending request to the background daemon via IPC
        let method = match tool_name {
            "codegraph_explore" => "explore",
            "codegraph_find_utility" => "find_utility",
            _ => return Err(format!("Unknown tool: {}", tool_name).into()),
        };

        match self.client.send_request(method, args.clone()).await {
            Ok(res) => Ok(res),
            Err(_) => {
                // 2. Fallback: If daemon is not running, query an in-memory CodeGraph on demand
                self.handle_in_process_fallback(tool_name, args).await
            }
        }
    }

    async fn handle_in_process_fallback(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let mut fallback_guard = self.fallback_graph.write().await;
        if fallback_guard.is_none() {
            info!("Daemon unavailable. Booting in-process fallback CodeGraph...");
            let mut graph = CodeGraph::new(false);
            for entry in WalkDir::new(&self.repo_root).into_iter().filter_map(|e| e.ok()) {
                let path = entry.path();
                if entry.file_type().is_file() {
                    let path_str = path.to_string_lossy();
                    if path_str.contains("/.git/")
                        || path_str.contains("/node_modules/")
                        || path_str.contains("/target/")
                    {
                        continue;
                    }
                    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                        if matches!(ext, "ts" | "tsx" | "js" | "py" | "rs" | "go") {
                            if let Ok(content) = std::fs::read_to_string(path) {
                                let rel = path.strip_prefix(&self.repo_root).unwrap_or(path);
                                let _ = graph.add_file(rel, &content);
                            }
                        }
                    }
                }
            }
            *fallback_guard = Some(graph);
        }

        let graph = fallback_guard.as_ref().unwrap();

        match tool_name {
            "codegraph_explore" => {
                let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("symbol_context");
                let target = args.get("target").and_then(|v| v.as_str()).unwrap_or("");
                let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
                let buffer_content = args.get("buffer_content").and_then(|v| v.as_str());

                match action {
                    "symbol_context" => {
                        let path_hint = if target.contains('/') || target.contains('.') {
                            Some(Path::new(target))
                        } else {
                            None
                        };
                        if let Some(ctx) = query_with_shadow_overlay(graph, target, path_hint, buffer_content) {
                            Ok(serde_json::to_value(ctx)?)
                        } else {
                            Err(format!("Symbol '{}' not found in codebase graph", target).into())
                        }
                    }
                    "call_hierarchy" => {
                        let direction = args.get("direction").and_then(|v| v.as_str()).unwrap_or("incoming");
                        Ok(serde_json::to_value(graph.get_call_hierarchy(target, direction, depth))?)
                    }
                    "impacted_files" => {
                        Ok(serde_json::to_value(graph.get_impacted_files(target, depth))?)
                    }
                    _ => Err(format!("Unknown action: {}", action).into()),
                }
            }
            "codegraph_find_utility" => {
                let intent = args.get("intent").and_then(|v| v.as_str()).unwrap_or("");
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(3) as usize;
                Ok(serde_json::to_value(graph.find_utility(intent, limit))?)
            }
            _ => Err("Invalid tool".into()),
        }
    }

    /// Runs the Model Context Protocol (MCP) JSON-RPC loop over stdio
    pub async fn run_stdio(&self) -> Result<(), Box<dyn std::error::Error>> {
        let stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        let mut lines = BufReader::new(stdin).lines();

        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }

            let req: JsonRpcRequest = match serde_json::from_str(&line) {
                Ok(r) => r,
                Err(e) => {
                    let err_resp = JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: None,
                        result: None,
                        error: Some(serde_json::json!({
                            "code": -32700,
                            "message": format!("Parse error: {}", e)
                        })),
                    };
                    let out = serde_json::to_string(&err_resp)? + "\n";
                    stdout.write_all(out.as_bytes()).await?;
                    stdout.flush().await?;
                    continue;
                }
            };

            // Process MCP method
            let resp = match req.method.as_str() {
                "initialize" => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: req.id,
                    result: Some(serde_json::json!({
                        "protocolVersion": "2024-11-05",
                        "capabilities": {
                            "tools": {}
                        },
                        "serverInfo": {
                            "name": "codegraph-mcp",
                            "version": "0.1.0"
                        }
                    })),
                    error: None,
                },
                "notifications/initialized" => {
                    continue; // Notifications do not generate responses
                }
                "tools/list" => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: req.id,
                    result: Some(serde_json::json!({
                        "tools": [
                            {
                                "name": "codegraph_explore",
                                "description": "Explores the structural codebase graph without reading raw files. Use this to inspect symbols, find callers/callees, or assess change impact.",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "action": {
                                            "type": "string",
                                            "enum": ["symbol_context", "call_hierarchy", "impacted_files"],
                                            "description": "The structural exploration operation to perform."
                                        },
                                        "target": {
                                            "type": "string",
                                            "description": "Symbol name or relative file path to inspect."
                                        },
                                        "depth": {
                                            "type": "integer",
                                            "default": 1,
                                            "description": "Traversal depth for call hierarchy and impact analysis (max: 3)."
                                        },
                                        "buffer_content": {
                                            "type": "string",
                                            "description": "Optional in-memory unsaved buffer text to inspect dirty edits."
                                        }
                                    },
                                    "required": ["action", "target"]
                                }
                            },
                            {
                                "name": "codegraph_find_utility",
                                "description": "MANDATORY before writing new helper functions or utility classes. Searches the repository for existing reusable logic matching functional intent.",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "intent": {
                                            "type": "string",
                                            "description": "Plain-English description of the desired behavior (e.g., 'exponential backoff retry', 'parse query params')."
                                        },
                                        "category": {
                                            "type": "string",
                                            "enum": ["function", "type", "class", "all"],
                                            "default": "all"
                                        },
                                        "limit": {
                                            "type": "integer",
                                            "default": 3
                                        }
                                    },
                                    "required": ["intent"]
                                }
                            }
                        ]
                    })),
                    error: None,
                },
                "tools/call" => {
                    let tool_name = req
                        .params
                        .as_ref()
                        .and_then(|p| p.get("name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or("");
                    let arguments = req
                        .params
                        .as_ref()
                        .and_then(|p| p.get("arguments"))
                        .cloned()
                        .unwrap_or(serde_json::json!({}));

                    match self.handle_tool_call(tool_name, arguments).await {
                        Ok(data) => JsonRpcResponse {
                            jsonrpc: "2.0".to_string(),
                            id: req.id,
                            result: Some(serde_json::json!({
                                "content": [
                                    {
                                        "type": "text",
                                        "text": serde_json::to_string_pretty(&data).unwrap_or_default()
                                    }
                                ]
                            })),
                            error: None,
                        },
                        Err(e) => JsonRpcResponse {
                            jsonrpc: "2.0".to_string(),
                            id: req.id,
                            result: Some(serde_json::json!({
                                "isError": true,
                                "content": [
                                    {
                                        "type": "text",
                                        "text": format!("Tool error: {}", e)
                                    }
                                ]
                            })),
                            error: None,
                        },
                    }
                }
                _ => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: req.id,
                    result: None,
                    error: Some(serde_json::json!({
                        "code": -32601,
                        "message": format!("Method not found: {}", req.method)
                    })),
                },
            };

            let out = serde_json::to_string(&resp)? + "\n";
            stdout.write_all(out.as_bytes()).await?;
            stdout.flush().await?;
        }

        Ok(())
    }
}
