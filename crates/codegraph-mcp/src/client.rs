use codegraph_security::auth::get_socket_path_for_repo;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

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

pub struct DaemonClient {
    pub socket_path: PathBuf,
}

impl DaemonClient {
    pub fn for_repo(repo_root: &Path) -> Result<Self, std::io::Error> {
        let socket_path = get_socket_path_for_repo(repo_root)?;
        Ok(Self { socket_path })
    }

    /// Sends a request to the background daemon and returns the response
    pub async fn send_request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        if !self.socket_path.exists() {
            return Err(format!(
                "CodeGraph daemon is not running. Start it with 'codegraph daemon' or run 'codegraph index'."
            )
            .into());
        }

        let stream = UnixStream::connect(&self.socket_path).await?;
        let (reader, mut writer) = stream.into_split();

        let req = IpcRequest {
            id: 1,
            method: method.to_string(),
            params,
        };

        let mut req_str = serde_json::to_string(&req)?;
        req_str.push('\n');
        writer.write_all(req_str.as_bytes()).await?;

        let mut lines = BufReader::new(reader).lines();
        if let Some(resp_line) = lines.next_line().await? {
            let resp: IpcResponse = serde_json::from_str(&resp_line)?;
            if let Some(err) = resp.error {
                return Err(err.into());
            }
            Ok(resp.result.unwrap_or(serde_json::Value::Null))
        } else {
            Err("Empty response from codegraph daemon".into())
        }
    }
}
