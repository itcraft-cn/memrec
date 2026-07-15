use anyhow::{Context, Result};
use memrec_common::{JsonRpcRequest, JsonRpcResponse};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

const INITIAL_BUFFER_SIZE: usize = 8192;
const MAX_BUFFER_SIZE: usize = 1024 * 1024; // 1MB

pub struct Client {
    socket_path: PathBuf,
}

impl Client {
    pub fn new() -> Result<Self> {
        let socket_path = Self::default_socket_path()?;
        Ok(Self { socket_path })
    }

    fn default_socket_path() -> Result<PathBuf> {
        let home = dirs::home_dir().context("Failed to get home directory")?;
        Ok(home.join(".memrec").join("memrecd.sock"))
    }

    pub async fn send(&self, request: &JsonRpcRequest) -> Result<JsonRpcResponse> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .await
            .context("Failed to connect to memrecd")?;

        let request_json = serde_json::to_string(request).context("Failed to serialize request")?;

        stream
            .write_all(request_json.as_bytes())
            .await
            .context("Failed to send request")?;

        stream.flush().await.context("Failed to flush stream")?;

        stream
            .shutdown()
            .await
            .context("Failed to shutdown stream")?;

        let mut buffer = Vec::with_capacity(INITIAL_BUFFER_SIZE);
        let mut chunk = vec![0u8; INITIAL_BUFFER_SIZE];

        loop {
            let n = stream
                .read(&mut chunk)
                .await
                .context("Failed to read response")?;

            if n == 0 {
                break;
            }

            buffer.extend_from_slice(&chunk[..n]);

            if buffer.len() >= MAX_BUFFER_SIZE {
                return Err(anyhow::anyhow!("Response too large (exceeds 1MB)"));
            }
        }

        let response_json = String::from_utf8_lossy(&buffer);
        let response: JsonRpcResponse =
            serde_json::from_str(&response_json).context("Failed to parse response")?;

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_default_socket_path() {
        let path = Client::default_socket_path().unwrap();
        assert!(path.to_string_lossy().contains(".memrec"));
    }
}
