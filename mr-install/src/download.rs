use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Sha256, Digest};
use std::path::{Path, PathBuf};

const MODEL_REPO: &str = "Qdrant/all-MiniLM-L6-v2-onnx";
const MODEL_FILES: &[&str] = &[
    "model.onnx",
    "tokenizer.json",
    "config.json",
    "special_tokens_map.json",
    "tokenizer_config.json",
];

const MODEL_HASHES: &[(&str, &str)] = &[
    ("model.onnx", "bbd7b466f6d58e646fdc2bd5fd67b2f5e93c0b687011bd4548c420f7bd46f0c5"),
    ("tokenizer.json", "da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0"),
    ("config.json", "1b4d8e2a3988377ed8b519a31d8d31025a25f1c5f8606998e8014111438efcd7"),
    ("special_tokens_map.json", "5d5b662e421ea9fac075174bb0688ee0d9431699900b90662acd44b2a350503a"),
    ("tokenizer_config.json", "bd2e06a5b20fd1b13ca988bedc8763d332d242381b4fbc98f8fead4524158f79"),
];

const HF_BASE_URL: &str = "https://huggingface.co";
const HF_MIRROR_BASE_URL: &str = "https://hf-mirror.com";

pub struct DownloadOptions {
    pub use_hf_mirror: bool,
    pub mirror_base_url: Option<String>,
    pub skip_hash_verify: bool,
}

fn build_base_url(opts: &DownloadOptions) -> String {
    if let Some(ref url) = opts.mirror_base_url {
        return url.trim_end_matches('/').to_string();
    }
    if opts.use_hf_mirror {
        return HF_MIRROR_BASE_URL.to_string();
    }
    HF_BASE_URL.to_string()
}

fn model_dir() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Failed to get home directory"))?;
    Ok(home.join(".memrec/models").join(MODEL_REPO.replace('/', "--")))
}

fn all_files_exist(model_dir: &Path) -> bool {
    MODEL_FILES.iter().all(|f| model_dir.join(f).exists())
}

fn get_file_hash(filename: &str) -> Option<&str> {
    MODEL_HASHES.iter()
        .find(|(name, _)| name == &filename)
        .map(|(_, hash)| *hash)
}

fn verify_file_hash(file_path: &Path, expected_hash: &str) -> Result<bool> {
    let mut file = std::fs::File::open(file_path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    let result = hasher.finalize();
    let actual_hash = hex::encode(result);
    Ok(actual_hash == expected_hash)
}

async fn download_file(url: &str, dest: &Path, pb: &ProgressBar) -> Result<()> {
    let response = reqwest::get(url).await
        .with_context(|| format!("Failed to connect to {}", url))?;
    
    if !response.status().is_success() {
        anyhow::bail!("HTTP {} for {}", response.status(), url);
    }
    
    let total_size = response.content_length();
    
    if let Some(size) = total_size {
        pb.set_length(size);
    }
    
    let mut file = std::fs::File::create(dest)
        .with_context(|| format!("Failed to create file {:?}", dest))?;
    
    let mut stream = response.bytes_stream();
    use futures_util::StreamExt;
    use std::io::Write;
    
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk)?;
        pb.inc(chunk.len() as u64);
    }
    
    Ok(())
}

async fn attempt_download_file(url: &str, dest: &Path, filename: &str) -> Result<()> {
    let pb = ProgressBar::new(0);
    pb.set_style(ProgressStyle::with_template(
        "  {msg} {spinner:.green} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})"
    )?.progress_chars("#>-"));
    pb.set_message(filename.to_string());
    
    download_file(url, dest, &pb).await
}

pub async fn download_model(opts: &DownloadOptions) -> Result<PathBuf> {
    let model_dir = model_dir()?;
    
    if all_files_exist(&model_dir) {
        println!("  Model files already exist in {}", model_dir.display());
        return Ok(model_dir);
    }
    
    std::fs::create_dir_all(&model_dir)?;
    
    let base_url = build_base_url(opts);
    
    println!("  Downloading from {} ...", base_url);
    
    for filename in MODEL_FILES.iter() {
        let dest = model_dir.join(filename);
        
        let expected_hash = get_file_hash(filename).unwrap_or("");
        
        if dest.exists() {
            if opts.skip_hash_verify {
                println!("  [warning] {} exists but hash verification skipped (security risk)", filename);
                continue;
            } else if verify_file_hash(&dest, expected_hash)? {
                println!("  [skip] {} (already exists and verified)", filename);
                continue;
            } else {
                println!("  [re-download] {} (hash mismatch)", filename);
                std::fs::remove_file(&dest).ok();
            }
        }
        
        let primary_url = format!("{}/{}/resolve/main/{}", base_url, MODEL_REPO, filename);
        let mut download_success = false;
        
        // 尝试主要URL
        match attempt_download_file(&primary_url, &dest, filename).await {
            Ok(_) => {
                if opts.skip_hash_verify {
                    println!("  [warning] {} downloaded without hash verification (security risk)", filename);
                    download_success = true;
                } else if verify_file_hash(&dest, expected_hash)? {
                    println!("  [ok] {} (verified)", filename);
                    download_success = true;
                } else {
                    println!("  [error] {} hash mismatch from primary source", filename);
                    std::fs::remove_file(&dest).ok();
                }
            }
            Err(e) => {
                println!("  [error] {} failed from primary: {}", filename, e);
            }
        }
        
        // 如果主要失败且未指定镜像，尝试hf-mirror
        if !download_success && !opts.use_hf_mirror && opts.mirror_base_url.is_none() {
            println!("  [retry] {} trying hf-mirror.com ...", filename);
            let mirror_url = format!("{}/{}/resolve/main/{}", HF_MIRROR_BASE_URL, MODEL_REPO, filename);
            
            match attempt_download_file(&mirror_url, &dest, filename).await {
                Ok(_) => {
                    if opts.skip_hash_verify {
                        println!("  [warning] {} downloaded via mirror without hash verification (security risk)", filename);
                        download_success = true;
                    } else if verify_file_hash(&dest, expected_hash)? {
                        println!("  [ok] {} (via mirror, verified)", filename);
                        download_success = true;
                    } else {
                        println!("  [error] {} hash mismatch from mirror", filename);
                        std::fs::remove_file(&dest).ok();
                    }
                }
                Err(e) => {
                    println!("  [error] {} failed from mirror: {}", filename, e);
                }
            }
        }
        
        if !download_success {
            anyhow::bail!("Failed to download {} with correct hash from any source", filename);
        }
    }
    
    println!("  Model download complete: {}", model_dir.display());
    Ok(model_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_build_base_url_default() {
        let opts = DownloadOptions {
            use_hf_mirror: false,
            mirror_base_url: None,
            skip_hash_verify: false,
        };
        assert_eq!(build_base_url(&opts), "https://huggingface.co");
    }
    
    #[test]
    fn test_build_base_url_hf_mirror() {
        let opts = DownloadOptions {
            use_hf_mirror: true,
            mirror_base_url: None,
            skip_hash_verify: false,
        };
        assert_eq!(build_base_url(&opts), "https://hf-mirror.com");
    }
    
    #[test]
    fn test_build_base_url_custom_mirror() {
        let opts = DownloadOptions {
            use_hf_mirror: false,
            mirror_base_url: Some("https://my-mirror.example.com/".to_string()),
            skip_hash_verify: false,
        };
        assert_eq!(build_base_url(&opts), "https://my-mirror.example.com");
    }
    
    #[test]
    fn test_build_base_url_custom_mirror_takes_priority() {
        let opts = DownloadOptions {
            use_hf_mirror: true,
            mirror_base_url: Some("https://custom.example.com".to_string()),
            skip_hash_verify: false,
        };
        assert_eq!(build_base_url(&opts), "https://custom.example.com");
    }
    
    #[test]
    fn test_model_dir_path() {
        let dir = model_dir().unwrap();
        let path = dir.to_string_lossy();
        assert!(path.contains(".memrec/models"));
        assert!(path.contains("Qdrant--all-MiniLM-L6-v2-onnx"));
    }
    
    #[test]
    fn test_all_files_exist_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!all_files_exist(dir.path()));
    }
    
    #[test]
    fn test_all_files_exist_with_files() {
        let dir = tempfile::tempdir().unwrap();
        for f in MODEL_FILES {
            std::fs::write(dir.path().join(f), "test").unwrap();
        }
        assert!(all_files_exist(dir.path()));
    }
    
    #[test]
    fn test_all_files_exist_partial() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("model.onnx"), "test").unwrap();
        assert!(!all_files_exist(dir.path()));
    }
    
    #[test]
    fn test_get_file_hash() {
        assert_eq!(get_file_hash("model.onnx"), Some("bbd7b466f6d58e646fdc2bd5fd67b2f5e93c0b687011bd4548c420f7bd46f0c5"));
        assert_eq!(get_file_hash("tokenizer.json"), Some("da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0"));
        assert_eq!(get_file_hash("nonexistent.txt"), None);
    }
    
    #[test]
    fn test_verify_file_hash() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "test content").unwrap();
        
        let wrong_hash = "0000000000000000000000000000000000000000000000000000000000000000";
        
        assert!(!verify_file_hash(&file_path, wrong_hash).unwrap());
        
        let mut hasher = Sha256::new();
        hasher.update("test content");
        let actual_hash = hex::encode(hasher.finalize());
        assert!(verify_file_hash(&file_path, &actual_hash).unwrap());
    }
}