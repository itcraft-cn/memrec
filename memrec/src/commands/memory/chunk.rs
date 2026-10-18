//! 长内容分块与分块标签工具。


/// 按最大尺寸拆分内容为多个块。
pub(super) fn split_content(content: &str, max_size: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut start = 0;

    while start < content.len() {
        let end = find_chunk_boundary(content, start, max_size);
        chunks.push(content[start..end].to_string());
        start = end;
    }

    chunks
}

/// 查找分块边界，优先在句子结束符处拆分。
///
/// 拆分优先级：句号/感叹号/问号/换行 → UTF-8 字符边界。
/// 每块至少 1KB 内容。
pub(super) fn find_chunk_boundary(content: &str, start: usize, max_size: usize) -> usize {
    let max_end = std::cmp::min(start + max_size, content.len());

    if max_end == content.len() {
        return max_end;
    }

    // 尝试在句子边界拆分（优先）
    let chunk = &content[start..max_end];
    let sentence_ends = ['.', '!', '?', '\n', '。', '！', '？'];

    for end_char in sentence_ends.iter().rev() {
        if let Some(pos) = chunk.rfind(*end_char) {
            let boundary = start + pos + end_char.len_utf8();
            if boundary > start + 1000 {
                // 至少1KB
                return boundary;
            }
        }
    }

    // 按UTF-8字符边界拆分
    let mut boundary = max_end;
    while boundary > start && !content.is_char_boundary(boundary) {
        boundary -= 1;
    }

    boundary
}

/// 为分块添加 part 标签（`part:N-M`、`part:first`、`part:last`）。
pub(super) fn format_part_tags(original_tags: &[String], part_num: usize, total_parts: usize) -> Vec<String> {
    let mut tags = original_tags.to_vec();
    tags.push(format!("part:{}-{}", part_num, total_parts));
    if part_num == 1 {
        tags.push("part:first".to_string());
    }
    if part_num == total_parts {
        tags.push("part:last".to_string());
    }
    tags
}
