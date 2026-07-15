use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use memrec_common::{ModelConfig, ModelFile};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

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

fn model_dir_for_config(model_config: &ModelConfig) -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Failed to get home directory"))?;
    Ok(home
        .join(".memrec/models")
        .join(model_config.local_dir_name()))
}

fn all_files_exist(model_dir: &Path, files: &[ModelFile]) -> bool {
    files
        .iter()
        .filter(|f| f.required)
        .all(|f| model_dir.join(&f.filename).exists())
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
    let response = reqwest::get(url)
        .await
        .with_context(|| format!("Failed to connect to {}", url))?;

    if !response.status().is_success() {
        anyhow::bail!("HTTP {} for {}", response.status(), url);
    }

    let total_size = response.content_length();

    let pb = ProgressBar::new(total_size.unwrap_or(0));
    pb.set_style(
        ProgressStyle::with_template(
            "  {msg} {spinner:.green} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})",
        )?
        .progress_chars("#>-"),
    );
    pb.set_message(filename.to_string());

    let mut file =
        std::fs::File::create(dest).with_context(|| format!("Failed to create file {:?}", dest))?;

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

fn is_placeholder_hash(hash: &str) -> bool {
    hash.chars().all(|c| c == '0')
}

pub async fn download_model(model_config: &ModelConfig, opts: &DownloadOptions) -> Result<PathBuf> {
    let model_dir = model_dir_for_config(model_config)?;
    let repo = model_config
        .huggingface_repo()
        .ok_or_else(|| anyhow::anyhow!("No HuggingFace repo configured for this model"))?;

    if all_files_exist(&model_dir, &model_config.files) {
        println!("  Model files already exist in {}", model_dir.display());
        return Ok(model_dir);
    }

    std::fs::create_dir_all(&model_dir)?;

    let base_url = build_base_url(opts);

    println!("  Downloading {} from {} ...", repo, base_url);

    for model_file in &model_config.files {
        if !model_file.required {
            continue;
        }

        let filename = &model_file.filename;
        let remote_path = &model_file.remote_path;
        let expected_hash = &model_file.sha256;
        let dest = model_dir.join(filename);

        if dest.exists() {
            if is_placeholder_hash(expected_hash) {
                println!(
                    "  [skip] {} (exists, hash verification unavailable)",
                    filename
                );
                continue;
            }
            if opts.skip_hash_verify {
                println!(
                    "  [warning] {} exists but hash verification skipped (security risk)",
                    filename
                );
                continue;
            } else if verify_file_hash(&dest, expected_hash)? {
                println!("  [skip] {} (already exists and verified)", filename);
                continue;
            } else {
                println!("  [re-download] {} (hash mismatch)", filename);
                std::fs::remove_file(&dest).ok();
            }
        }

        let primary_url = format!("{}/{}/resolve/main/{}", base_url, repo, remote_path);
        let mut download_success = false;

        match attempt_download_file(&primary_url, &dest, filename).await {
            Ok(_) => {
                if is_placeholder_hash(expected_hash) || opts.skip_hash_verify {
                    println!(
                        "  [ok] {} (downloaded, hash verification {})",
                        filename,
                        if is_placeholder_hash(expected_hash) {
                            "unavailable"
                        } else {
                            "skipped"
                        }
                    );
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
            let mirror_url = format!(
                "{}/{}/resolve/main/{}",
                HF_MIRROR_BASE_URL, repo, remote_path
            );

            match attempt_download_file(&mirror_url, &dest, filename).await {
                Ok(_) => {
                    if is_placeholder_hash(expected_hash) || opts.skip_hash_verify {
                        println!(
                            "  [ok] {} (via mirror, hash verification {})",
                            filename,
                            if is_placeholder_hash(expected_hash) {
                                "unavailable"
                            } else {
                                "skipped"
                            }
                        );
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
            anyhow::bail!(
                "Failed to download {} with correct hash from any source",
                filename
            );
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
    fn test_model_dir_for_minilm() {
        let config = ModelConfig::default();
        let dir = model_dir_for_config(&config).unwrap();
        let path = dir.to_string_lossy();
        assert!(path.contains(".memrec/models"));
        assert!(path.contains("Qdrant--all-MiniLM-L6-v2-onnx"));
    }

    #[test]
    fn test_model_dir_for_bge_m3_model_dir() {
        let config = ModelConfig::new(memrec_common::ModelType::BGEM3);
        let dir = model_dir_for_config(&config).unwrap();
        let path = dir.to_string_lossy();
        assert!(path.contains(".memrec/models"));
        assert!(path.contains("BAAI--bge-m3"));
    }

    #[test]
    fn test_all_files_exist_empty_dir() {
        let config = ModelConfig::default();
        let dir = tempfile::tempdir().unwrap();
        assert!(!all_files_exist(dir.path(), &config.files));
    }

    #[test]
    fn test_all_files_exist_with_files() {
        let config = ModelConfig::default();
        let dir = tempfile::tempdir().unwrap();
        for f in &config.files {
            if f.required {
                std::fs::write(dir.path().join(&f.filename), "test").unwrap();
            }
        }
        assert!(all_files_exist(dir.path(), &config.files));
    }

    #[test]
    fn test_all_files_exist_partial() {
        let config = ModelConfig::default();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("model.onnx"), "test").unwrap();
        assert!(!all_files_exist(dir.path(), &config.files));
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

    #[test]
    fn test_is_placeholder_hash() {
        assert!(is_placeholder_hash(
            "0000000000000000000000000000000000000000000000000000000000000000"
        ));
        assert!(!is_placeholder_hash(
            "bbd7b466f6d58e646fdc2bd5fd67b2f5e93c0b687011bd4548c420f7bd46f0c5"
        ));
    }

    #[test]
    fn test_bge_m3_remote_paths() {
        let config = ModelConfig::new(memrec_common::ModelType::BGEM3);
        let onnx_file = config.onnx_model_file().unwrap();
        assert_eq!(onnx_file.remote_path, "onnx/model.onnx");
        assert_eq!(onnx_file.filename, "model.onnx");

        let ext_files = config.external_data_files();
        assert_eq!(ext_files.len(), 2);
        assert_eq!(ext_files[0].remote_path, "onnx/model.onnx_data");
        assert_eq!(ext_files[0].filename, "model.onnx_data");
        assert_eq!(ext_files[1].remote_path, "onnx/Constant_7_attr__value");
        assert_eq!(ext_files[1].filename, "Constant_7_attr__value");
    }
}
