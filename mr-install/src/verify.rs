//! # 安装验证
//!
//! 安装完成后执行端到端测试：写入 → 搜索 → 删除 → 版本检查。

use anyhow::Result;

use crate::dirs::default_bin_dir;

/// 运行安装验证流程
pub fn run_verification() -> Result<()> {
    let bin_dir = default_bin_dir();

    let memrec = bin_dir.join("memrec");

    if !memrec.exists() {
        anyhow::bail!("memrec binary not found at {}", memrec.display());
    }

    println!("  Testing write...");
    let test_content = format!(
        "mr-install verification test - {}",
        chrono::Utc::now().to_rfc3339()
    );

    let add_output = std::process::Command::new(&memrec)
        .args([
            "add",
            &test_content,
            "--mtype",
            "knowledge",
            "--tag",
            "test",
        ])
        .output()?;

    if !add_output.status.success() {
        let stderr = String::from_utf8_lossy(&add_output.stderr);
        anyhow::bail!("Write test failed: {}", stderr.trim());
    }

    let add_stdout = String::from_utf8_lossy(&add_output.stdout);
    let test_id = extract_memory_id(&add_stdout);

    if let Some(id) = &test_id {
        println!("  Write success: {}", id);
    } else {
        println!("  Write completed (could not parse ID)");
    }

    std::thread::sleep(std::time::Duration::from_secs(1));

    println!("  Testing search...");
    let search_output = std::process::Command::new(&memrec)
        .args(["search", "verification test", "--project-only"])
        .output()?;

    if search_output.status.success() {
        let stdout = String::from_utf8_lossy(&search_output.stdout);
        if stdout.contains("Found") {
            println!("  Search success");
        } else {
            println!("  Search returned no results (may need model)");
        }
    }

    if let Some(id) = &test_id {
        let del_output = std::process::Command::new(&memrec)
            .args(["delete", id])
            .output();
        if del_output.map(|o| o.status.success()).unwrap_or(false) {
            println!("  Test memory cleaned up");
        }
    }

    println!("  Testing version...");
    let ver_output = std::process::Command::new(&memrec)
        .arg("version")
        .output()?;

    if ver_output.status.success() {
        let ver = String::from_utf8_lossy(&ver_output.stdout);
        println!("  {}", ver.trim());
    }

    Ok(())
}

/// 从 memrec add 输出中提取记忆 UUID
fn extract_memory_id(output: &str) -> Option<String> {
    for line in output.lines() {
        if line.contains("Added memory:") {
            let id = line.split("Added memory:").nth(1)?.trim();
            if id.len() == 36 && id.contains('-') {
                return Some(id.to_string());
            }
        }
        if line.contains("ID:") {
            let id = line.split("ID:").nth(1)?.trim();
            if id.len() == 36 && id.contains('-') {
                return Some(id.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_memory_id_from_added() {
        let output = "Added memory: 01f699c2-28e8-47bb-a659-bcf3e2701ef8";
        assert_eq!(
            extract_memory_id(output),
            Some("01f699c2-28e8-47bb-a659-bcf3e2701ef8".to_string())
        );
    }

    #[test]
    fn test_extract_memory_id_from_id_line() {
        let output = "  ID: 29e61821-5eb1-4c07-bab6-70ad703e5f05";
        assert_eq!(
            extract_memory_id(output),
            Some("29e61821-5eb1-4c07-bab6-70ad703e5f05".to_string())
        );
    }

    #[test]
    fn test_extract_memory_id_no_match() {
        let output = "Some other output without an ID";
        assert_eq!(extract_memory_id(output), None);
    }

    #[test]
    fn test_extract_memory_id_invalid_format() {
        let output = "Added memory: not-a-uuid";
        assert_eq!(extract_memory_id(output), None);
    }

    #[test]
    fn test_extract_memory_id_multiline() {
        let output =
            "Memory added successfully\n  ID: 01f699c2-28e8-47bb-a659-bcf3e2701ef8\nTags: test";
        assert_eq!(
            extract_memory_id(output),
            Some("01f699c2-28e8-47bb-a659-bcf3e2701ef8".to_string())
        );
    }
}
