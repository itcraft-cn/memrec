use anyhow::{Result, Context};
use tokio::net::UnixStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::path::PathBuf;
use memrec_common::{JsonRpcRequest, JsonRpcResponse};

pub struct Client {
    socket_path: PathBuf,
}

impl Client {
    pub fn new() -> Result<Self> {
        let socket_path = Self::default_socket_path()?;
        Ok(Self { socket_path })
    }
    
    fn default_socket_path() -> Result<PathBuf> {
        let home = dirs::home_dir()
            .context("Failed to get home directory")?;
        Ok(home.join(".memrec").join("memrecd.sock"))
    }
    
    pub async fn send(&self, request: &JsonRpcRequest) -> Result<JsonRpcResponse> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .await
            .context("Failed to connect to memrecd")?;
        
        let request_json = serde_json::to_string(request)
            .context("Failed to serialize request")?;
        
        stream.write_all(request_json.as_bytes())
            .await
            .context("Failed to send request")?;
        
        stream.flush()
            .await
            .context("Failed to flush stream")?;
        
        let mut buffer = vec![0u8; 8192];
        let n = stream.read(&mut buffer)
            .await
            .context("Failed to read response")?;
        
        let response_json = String::from_utf8_lossy(&buffer[..n]);
        let response: JsonRpcResponse = serde_json::from_str(&response_json)
            .context("Failed to parse response")?;
        
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