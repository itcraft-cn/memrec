use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};

const MODEL_REPO: &str = "Qdrant/all-MiniLM-L6-v2-onnx";
const MODEL_FILES: &[&str] = &[
    "model.onnx",
    "tokenizer.json",
    "config.json",
    "special_tokens_map.json",
    "tokenizer_config.json",
];

const HF_BASE_URL: &str = "https://huggingface.co";
const HF_MIRROR_BASE_URL: &str = "https://hf-mirror.com";

pub struct DownloadOptions {
    pub use_hf_mirror: bool,
    pub mirror_base_url: Option<String>,
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
    let urls: Vec<(String, String)> = MODEL_FILES.iter().map(|f| {
        let url = format!("{}/{}/resolve/main/{}", base_url, MODEL_REPO, f);
        (url, f.to_string())
    }).collect();
    
    println!("  Downloading from {} ...", base_url);
    
    for (url, filename) in &urls {
        let dest = model_dir.join(filename);
        
        if dest.exists() {
            println!("  [skip] {} (already exists)", filename);
            continue;
        }
        
        let pb = ProgressBar::new(0);
        pb.set_style(ProgressStyle::with_template(
            "  {msg} {spinner:.green} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})"
        )?.progress_chars("#>-"));
        pb.set_message(filename.to_string());
        
        match download_file(url, &dest, &pb).await {
            Ok(()) => println!("  [ok] {}", filename),
            Err(e) => {
                std::fs::remove_file(&dest).ok();
                
                if !opts.use_hf_mirror && opts.mirror_base_url.is_none() {
                    println!("  [retry] {} failed from huggingface, trying hf-mirror.com ...", filename);
                    let mirror_url = format!("{}/{}/resolve/main/{}", HF_MIRROR_BASE_URL, MODEL_REPO, filename);
                    let pb2 = ProgressBar::new(0);
                    pb2.set_style(ProgressStyle::with_template(
                        "  {msg} {spinner:.green} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})"
                    )?.progress_chars("#>-"));
                    pb2.set_message(format!("{} (mirror)", filename));
                    
                    match download_file(&mirror_url, &dest, &pb2).await {
                        Ok(()) => println!("  [ok] {} (via mirror)", filename),
                        Err(e2) => {
                            std::fs::remove_file(&dest).ok();
                            anyhow::bail!(
                                "Failed to download {} from both huggingface and hf-mirror:\n  primary: {}\n  mirror: {}",
                                filename, e, e2
                            );
                        }
                    }
                } else {
                    anyhow::bail!("Failed to download {}: {}", filename, e);
                }
            }
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
        };
        assert_eq!(build_base_url(&opts), "https://huggingface.co");
    }
    
    #[test]
    fn test_build_base_url_hf_mirror() {
        let opts = DownloadOptions {
            use_hf_mirror: true,
            mirror_base_url: None,
        };
        assert_eq!(build_base_url(&opts), "https://hf-mirror.com");
    }
    
    #[test]
    fn test_build_base_url_custom_mirror() {
        let opts = DownloadOptions {
            use_hf_mirror: false,
            mirror_base_url: Some("https://my-mirror.example.com/".to_string()),
        };
        assert_eq!(build_base_url(&opts), "https://my-mirror.example.com");
    }
    
    #[test]
    fn test_build_base_url_custom_mirror_takes_priority() {
        let opts = DownloadOptions {
            use_hf_mirror: true,
            mirror_base_url: Some("https://custom.example.com".to_string()),
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
}
