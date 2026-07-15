use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Sha256, Digest};
use std::path::{Path, PathBuf};

const MINILM_MODEL_REPO: &str = "Qdrant/all-MiniLM-L6-v2-onnx";
const MINILM_MODEL_FILES: &[(&str, &str)] = &[
    ("model.onnx", "bbd7b466f6d58e646fdc2bd5fd67b2f5e93c0b687011bd4548c420f7bd46f0c5"),
    ("tokenizer.json", "da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0"),
    ("config.json", "1b4d8e2a3988377ed8b519a31d8d31025a25f1c5f8606998e8014111438efcd7"),
    ("special_tokens_map.json", "5d5b662e421ea9fac075174bb0688ee0d9431699900b90662acd44b2a350503a"),
    ("tokenizer_config.json", "bd2e06a5b20fd1b13ca988bedc8763d332d242381b4fbc98f8fead4524158f79"),
];

const BGEM3_MODEL_REPO: &str = "BAAI/bge-m3";
const BGEM3_MODEL_FILES: &[(&str, &str)] = &[
    ("model.onnx", "0000000000000000000000000000000000000000000000000000000000000000"),
    ("model.onnx_data", "0000000000000000000000000000000000000000000000000000000000000000"),
    ("sentencepiece.bpe.model", "0000000000000000000000000000000000000000000000000000000000000000"),
    ("tokenizer.json", "0000000000000000000000000000000000000000000000000000000000000000"),
    ("tokenizer_config.json", "7e4c1cc848840aeccdd763458c18dd525eb0f795c992e00ebe9c28554e7db2d4"),
    ("special_tokens_map.json", "8c785abebea9ae3257b61681b4e6fd8365ceafde980c21970d001e834cf10835"),
    ("config.json", "f24afd5de914fba8c668426c43d208a1a54022500c63b2c160be20891686fce8"),
    ("Constant_7_attr__value", "0000000000000000000000000000000000000000000000000000000000000000"),
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
    Ok(home.join(".memrec/models").join(MINILM_MODEL_REPO.replace('/', "--")))
}

fn all_files_exist(model_dir: &Path) -> bool {
    MINILM_MODEL_FILES.iter().all(|(filename, _)| model_dir.join(filename).exists())
}

fn verify_file_hash(file_path: &Path, expected_hash: &str) -> Result<bool> {
    let mut file = std::fs::File::open(file_path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    let result = hasher.finalize();
    let actual_hash = hex::encode(result);
    Ok(actual_hash == expected_hash)
}

async fn attempt_download_file(url: &str, dest: &Path, filename: &str) -> Result<()> {
    let response = reqwest::get(url).await
        .with_context(|| format!("Failed to connect to {}", url))?;
    
    if !response.status().is_success() {
        anyhow::bail!("HTTP {} for {}", response.status(), url);
    }
    
    let total_size = response.content_length();
    
    let pb = ProgressBar::new(total_size.unwrap_or(0));
    pb.set_style(ProgressStyle::with_template(
        "  {msg} {spinner:.green} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})"
    )?.progress_chars("#>-"));
    pb.set_message(filename.to_string());
    
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
    
    pb.finish_and_clear();
    Ok(())
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
    
    for (filename, expected_hash) in MINILM_MODEL_FILES.iter() {
        let dest = model_dir.join(filename);
        
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
        
        let primary_url = format!("{}/{}/resolve/main/{}", base_url, MINILM_MODEL_REPO, filename);
        let mut download_success = false;
        
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
        
        if !download_success && !opts.use_hf_mirror && opts.mirror_base_url.is_none() {
            println!("  [retry] {} trying hf-mirror.com ...", filename);
            let mirror_url = format!("{}/{}/resolve/main/{}", HF_MIRROR_BASE_URL, MINILM_MODEL_REPO, filename);
            
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
        for (filename, _) in MINILM_MODEL_FILES {
            std::fs::write(dir.path().join(filename), "test").unwrap();
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
