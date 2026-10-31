//! Runtime Unix domain socket server and client.
//!
//! Follows `design/agent-run.md` §7, §9 and `specs/git-hardening/spec.md`.
//! Serves length-delimited JSON frames over `/run/factory/runtime.sock` (mode 0600)
//! for `factory-agent mcp`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

/// Frame length limit (16 MiB) to guard against corrupted frames.
pub const MAX_FRAME_SIZE: usize = 16 * 1024 * 1024;

/// Requests sent to the runtime Unix socket.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum RuntimeRequest {
    #[serde(rename = "mcp_list_tools")]
    McpListTools,
    #[serde(rename = "mcp_call_tool")]
    McpCallTool {
        name: String,
        arguments_json: String,
    },
}

/// Wire representation of a `ToolDescriptor` exchanged over the runtime socket.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpToolDescriptorWire {
    pub name: String,
    pub title: String,
    pub description: String,
    pub input_schema_json: String,
    pub output_schema_json: Option<String>,
    pub read_only: bool,
    pub destructive: bool,
    pub idempotent: bool,
    pub open_world: bool,
}

impl From<&df_proto::agent::v1::ToolDescriptor> for McpToolDescriptorWire {
    fn from(t: &df_proto::agent::v1::ToolDescriptor) -> Self {
        let (read_only, destructive, idempotent, open_world) = if let Some(a) = &t.annotations {
            (a.read_only, a.destructive, a.idempotent, a.open_world)
        } else {
            (false, false, false, false)
        };
        Self {
            name: t.name.clone(),
            title: t.title.clone(),
            description: t.description.clone(),
            input_schema_json: t.input_schema_json.clone(),
            output_schema_json: t.output_schema_json.clone(),
            read_only,
            destructive,
            idempotent,
            open_world,
        }
    }
}

/// Responses returned from the runtime Unix socket.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum RuntimeResponse {
    #[serde(rename = "mcp_list_tools")]
    McpListTools { tools: Vec<McpToolDescriptorWire> },
    #[serde(rename = "mcp_call_tool")]
    McpCallTool {
        call_id: String,
        is_error: bool,
        content: Vec<String>,
        structured_content_json: Option<String>,
        end_run_after_turn: bool,
        error_message: Option<String>,
    },
}

/// Writes a length-prefixed JSON frame.
pub async fn write_frame<W: AsyncWriteExt + Unpin, T: Serialize>(
    writer: &mut W,
    val: &T,
) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(val)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let len = frame_len(bytes.len())?;
    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}

/// Reads a length-prefixed JSON frame.
pub async fn read_frame<R: AsyncReadExt + Unpin, T: serde::de::DeserializeOwned>(
    reader: &mut R,
) -> std::io::Result<T> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_SIZE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("frame size {len} exceeds limit {MAX_FRAME_SIZE}"),
        ));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    let val: T = serde_json::from_slice(&buf)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(val)
}

/// The length prefix of a frame of `len` bytes.
fn frame_len(len: usize) -> std::io::Result<u32> {
    u32::try_from(len).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// Handler trait for processing incoming requests on the runtime socket.
#[tonic::async_trait]
pub trait RuntimeHandler: Send + Sync + 'static {
    async fn handle_mcp_list_tools(&self) -> Vec<df_proto::agent::v1::ToolDescriptor> {
        Vec::new()
    }

    async fn handle_mcp_call_tool(
        &self,
        _name: &str,
        _arguments_json: &str,
    ) -> Result<df_proto::agent::v1::ToolCallResult, String> {
        Err("not implemented".to_string())
    }
}

/// Runtime Unix domain socket server.
pub struct RuntimeSocketServer {
    socket_path: PathBuf,
    listener: UnixListener,
    handler: Arc<dyn RuntimeHandler>,
}

impl RuntimeSocketServer {
    /// Binds to `socket_path`, cleans up any stale socket, and sets permissions to 0600 on Unix.
    pub fn bind(
        socket_path: impl Into<PathBuf>,
        handler: Arc<dyn RuntimeHandler>,
    ) -> std::io::Result<Self> {
        let socket_path = socket_path.into();
        if socket_path.exists() {
            std::fs::remove_file(&socket_path).ok(); // best-effort cleanup
        }
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                // Best effort: the parent directory may belong to someone else.
                std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).ok();
            }
        }

        let listener = UnixListener::bind(&socket_path)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))?;
        }

        Ok(Self {
            socket_path,
            listener,
            handler,
        })
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Spawns the accept loop in the background and returns a handle; aborting it also drops all
    /// connection handlers.
    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted = self.listener.accept() => match accepted {
                        Ok((stream, _)) => {
                            connections.spawn(serve_connection(stream, Arc::clone(&self.handler)));
                        }
                        Err(e) => {
                            // Transient (EMFILE, ECONNABORTED, ...): keep serving
                            tracing::warn!(error = %e, "runtime socket accept failed");
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        }
                    },
                    Some(_) = connections.join_next(), if !connections.is_empty() => {}
                }
            }
        })
    }
}

/// Serves requests on one connection until the peer closes it or a write fails.
async fn serve_connection(mut stream: UnixStream, handler: Arc<dyn RuntimeHandler>) {
    while let Ok(req) = read_frame::<_, RuntimeRequest>(&mut stream).await {
        match req {
            RuntimeRequest::McpListTools => {
                let tools = handler.handle_mcp_list_tools().await;
                let resp = RuntimeResponse::McpListTools {
                    tools: tools.iter().map(McpToolDescriptorWire::from).collect(),
                };
                if write_frame(&mut stream, &resp).await.is_err() {
                    break;
                }
            }
            RuntimeRequest::McpCallTool {
                name,
                arguments_json,
            } => match handler.handle_mcp_call_tool(&name, &arguments_json).await {
                Ok(result) => {
                    let content_strings = result
                        .content
                        .iter()
                        .filter_map(|c| match &c.kind {
                            Some(df_proto::transcript::v1::content_block::Kind::Text(t)) => {
                                Some(t.text.clone())
                            }
                            _ => None,
                        })
                        .collect();
                    let structured_content_json = if result.structured_content_json.is_empty() {
                        None
                    } else {
                        Some(result.structured_content_json)
                    };
                    let resp = RuntimeResponse::McpCallTool {
                        call_id: result.call_id,
                        is_error: result.is_error,
                        content: content_strings,
                        structured_content_json,
                        end_run_after_turn: result.end_run_after_turn,
                        error_message: None,
                    };
                    if write_frame(&mut stream, &resp).await.is_err() {
                        break;
                    }
                }
                Err(err) => {
                    let resp = RuntimeResponse::McpCallTool {
                        call_id: String::new(),
                        is_error: true,
                        content: vec![err.clone()],
                        structured_content_json: None,
                        end_run_after_turn: false,
                        error_message: Some(err),
                    };
                    if write_frame(&mut stream, &resp).await.is_err() {
                        break;
                    }
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::frame_len;

    #[test]
    fn frame_length_beyond_the_u32_prefix_is_an_error() {
        assert_eq!(frame_len(7).unwrap(), 7);
        let err = frame_len(usize::try_from(u32::MAX).unwrap() + 1).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }
}
