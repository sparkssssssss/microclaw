use std::borrow::Cow;

#[derive(Debug, Clone)]
pub struct SearchItem {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

// ==========================================
// 安全切片辅助函数 (防止 Panic 核心逻辑)
// ==========================================

/// 安全切片：如果索引不在字符边界，自动向前调整；如果越界，返回空字符串
fn safe_slice(s: &str, start: usize, end: usize) -> &str {
    let len = s.len();
    // floor_char_boundary 确保索引落在字符开始位置
    let start = s.floor_char_boundary(start.min(len));
    let end = s.floor_char_boundary(end.min(len));
    
    if start >= end {
        return "";
    }
    
    // 使用 get 避免 panic
    s.get(start..end).unwrap_or("")
}

/// 安全切片（从 start 到末尾）
fn safe_slice_from(s: &str, start: usize) -> &str {
    let len = s.len();
    let start = s.floor_char_boundary(start.min(len));
    s.get(start..).unwrap_or("")
}

// ==========================================
// 核心逻辑函数
// ==========================================

fn strip_block(mut html: String, tag: &str) -> String {
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    
    // 循环清理所有匹配的块
    let mut search_start = 0;
    
    loop {
        let Some(start) = find_case_insensitive(&html, &open, search_start) else {
            break;
        };
        
        let Some(end) = find_case_insensitive(&html, &close, start) else {
            // 只有开始没有结束，为了安全，截断后续所有内容
            if html.is_char_boundary(start) {
                html.truncate(start);
            }
            break;
        };
        
        let remove_to = end + close.len();
        
        // 再次检查边界合法性
        if !html.is_char_boundary(start) || !html.is_char_boundary(remove_to) {
            // 如果计算出的位置非法，为了防止死循环，强制跳过当前位置
            search_start = start + 1;
            continue;
        }

        // 替换为一个空格
        html.replace_range(start..remove_to, " ");
        
        // 下次搜索从替换位置开始
        search_start = start;
    }
    html
}

/// 查找子串（不区分大小写），带死循环保护
fn find_case_insensitive(haystack: &str, needle: &str, from: usize) -> Option<usize> {
    if from >= haystack.len() {
        return None;
    }

    // 1. 安全起点（可能会因为 UTF-8 边界回退）
    let safe_start = haystack.floor_char_boundary(from);
    
    let slice = haystack.get(safe_start..)?;
    // 注意：to_ascii_lowercase 会分配新内存
    let lower_slice = slice.to_ascii_lowercase();
    let lower_needle = needle.to_ascii_lowercase();

    let mut search_offset = 0;
    
    loop {
        // 在子串中查找
        let match_idx = lower_slice[search_offset..].find(&lower_needle)?;
        
        // 计算绝对索引
        let absolute_idx = safe_start + search_offset + match_idx;
        
        // 2. 关键判定：如果因为 floor 回退导致找到了之前的标签，必须跳过
        if absolute_idx >= from {
            return Some(absolute_idx);
        }

        // 找到了旧数据，跳过它继续找
        search_offset += match_idx + 1;
        
        if search_offset >= lower_slice.len() {
            return None;
        }
    }
}

pub fn decode_html_entities(input: &str) -> Cow<'_, str> {
    if !input.contains('&') {
        return Cow::Borrowed(input);
    }

    let mut out = input.to_string();
    let replacements = [
        ("&nbsp;", " "),
        ("&amp;", "&"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&#39;", "'"),
        ("&apos;", "'"),
        ("&mdash;", "—"),
        ("&ndash;", "–"),
        ("&copy;", "©"),
    ];
    for (from, to) in replacements {
        out = out.replace(from, to);
    }
    Cow::Owned(out)
}

pub fn html_to_text(html: &str) -> String {
    let html = strip_block(strip_block(html.to_string(), "script"), "style");

    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => {
                in_tag = true;
                text.push(' ');
            }
            '>' => {
                in_tag = false;
            }
            _ if !in_tag => text.push(ch),
            _ => {}
        }
    }

    collapse_whitespace(&decode_html_entities(&text))
}

pub fn collapse_whitespace(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_ws = false;
    for ch in input.chars() {
        if ch.is_whitespace() {
            if !last_ws {
                out.push(' ');
                last_ws = true;
            }
        } else {
            out.push(ch);
            last_ws = false;
        }
    }

    let mut compact = String::with_capacity(out.len());
    let punctuation = ['.', ',', ':', ';', '!', '?'];
    for ch in out.trim().chars() {
        if punctuation.contains(&ch) && compact.ends_with(' ') {
            compact.pop();
        }
        compact.push(ch);
    }

    compact
}

fn extract_attr(tag: &str, attr: &str) -> Option<String> {
    let lower_tag = tag.to_ascii_lowercase();
    let needle = format!("{}=", attr.to_ascii_lowercase());
    let idx = lower_tag.find(&needle)?;
    
    // 安全切片
    let raw_start = idx + needle.len();
    let raw = safe_slice_from(tag, raw_start);
    let raw = raw.trim_start();
    
    if raw.is_empty() {
        return None;
    }

    let first = raw.chars().next()?;
    if first == '"' || first == '\'' {
        let quote = first;
        let rest = raw.get(1..)?; // 跳过引号
        let end = rest.find(quote)?;
        return Some(safe_slice(rest, 0, end).to_string());
    }

    let end = raw
        .find(|c: char| c.is_whitespace() || c == '>')
        .unwrap_or(raw.len());
    
    Some(safe_slice(raw, 0, end).to_string())
}

fn extract_snippet_near(html: &str, from: usize) -> String {
    let from = html.floor_char_boundary(from.min(html.len()));
    let window_end = html.floor_char_boundary(from.saturating_add(4000).min(html.len()));
    
    // 使用 safe_slice 替代 &html[...]
    let segment = safe_slice(html, from, window_end);
    
    let Some(class_idx) = find_case_insensitive(segment, "result__snippet", 0) else {
        return String::new();
    };

    let pre_segment = safe_slice(segment, 0, class_idx);
    let tag_start = pre_segment.rfind('<').unwrap_or(0);
    
    let search_segment = safe_slice_from(segment, tag_start);
    let Some(tag_end_rel) = search_segment.find('>') else {
        return String::new();
    };
    let tag_end = tag_start + tag_end_rel;

    let closing = if safe_slice_from(segment, tag_start).starts_with("<a") {
        "</a>"
    } else {
        "</div>"
    };

    let Some(close_rel) = find_case_insensitive(segment, closing, tag_end) else {
        return String::new();
    };

    let inner = safe_slice(segment, tag_end + 1, close_rel);
    html_to_text(inner)
}

pub fn extract_ddg_results(html: &str, max_results: usize) -> Vec<SearchItem> {
    let mut results = Vec::new();
    let mut pos = 0usize;
    
    // 循环安全计数器
    let mut loop_count = 0;
    const MAX_LOOP_LIMIT: usize = 100; 

    while results.len() < max_results {
        loop_count += 1;
        if loop_count > MAX_LOOP_LIMIT {
            // 超过100次循环还没找完，强制退出防止卡死
            break;
        }
        
        // 记录循环开始时的位置，用于检测是否原地踏步
        let start_pos = pos;

        let Some(a_start) = find_case_insensitive(html, "<a", pos) else {
            break;
        };
        
        let search_area = safe_slice_from(html, a_start);
        let Some(a_tag_end_rel) = search_area.find('>') else {
            break;
        };
        let a_tag_end = a_start + a_tag_end_rel;
        
        let a_tag = safe_slice(html, a_start, a_tag_end + 1);

        let class = extract_attr(a_tag, "class").unwrap_or_default();
        if !class.split_whitespace().any(|c| c == "result__a") {
            // 跳过非结果链接
            pos = a_tag_end + 1;
            
            // 确保 pos 前进
            if pos <= start_pos {
                pos = start_pos + 1;
            }
            continue;
        }

        let Some(close_rel) = find_case_insensitive(html, "</a>", a_tag_end + 1) else {
            break;
        };

        let href = extract_attr(a_tag, "href")
            .map(|h| decode_html_entities(&h).into_owned())
            .unwrap_or_default();
            
        let title_html = safe_slice(html, a_tag_end + 1, close_rel);
        let title = html_to_text(title_html);
        let snippet = extract_snippet_near(html, close_rel + 4);

        if !href.is_empty() && !title.is_empty() {
            results.push(SearchItem {
                title,
                url: href,
                snippet,
            });
        }

        pos = close_rel + 4;
        
        // 最终防御：如果 pos 没有增加，强制增加
        if pos <= start_pos {
            pos = start_pos + 1; 
        }
    }

    // 只有在结果为空时才打印日志，帮助排查问题
    if results.is_empty() {
        println!("[Warn] extract_ddg_results found 0 items. HTML len: {}. Preview: {:?}", 
            html.len(), 
            safe_slice(html, 0, 300) // 打印前300字符
        );
    }

    results
}

pub fn extract_primary_html(html: &str) -> &str {
    let candidates = ["main", "article", "body"];
    for tag in candidates {
        let open = format!("<{tag}");
        let close = format!("</{tag}>");
        if let Some(start) = find_case_insensitive(html, &open, 0) {
            let search_area = safe_slice_from(html, start);
            if let Some(open_end_rel) = search_area.find('>') {
                let content_start = start + open_end_rel + 1;
                
                if let Some(end) = find_case_insensitive(html, &close, content_start) {
                    return safe_slice(html, content_start, end);
                }
            }
        }
    }
    html
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_to_text() {
        let html = "<html><body><h1>Hello&nbsp;World</h1><script>x=1;</script></body></html>";
        assert_eq!(html_to_text(html), "Hello World");
    }

    #[test]
    fn test_extract_ddg_results() {
        let html = r#"
<div>
  <a class="result__a" href="https://example.com">Example <b>Title</b></a>
  <a class="result__snippet">This is <b>snippet</b>.</a>
</div>
"#;
        let items = extract_ddg_results(html, 8);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Example Title");
        assert_eq!(items[0].url, "https://example.com");
        assert_eq!(items[0].snippet, "This is snippet.");
    }

    #[test]
    fn test_extract_primary_html_prefers_main() {
        let html = "<body>body</body><main>main section</main>";
        assert_eq!(extract_primary_html(html), "main section");
    }

    #[test]
    fn test_find_case_insensitive_loop_fix() {
        // 测试死循环修复逻辑
        let s = "abc只def";
        // '只'在索引3,4,5
        // 从索引4开始查找 'def' (索引6)
        // 原始逻辑 floor(4)->3, find('只')->3, 3 < 4, 死循环
        // 新逻辑应该跳过3，找到6
        assert_eq!(find_case_insensitive(s, "def", 4), Some(6));
    }
    
    #[test]
    fn test_safe_slice() {
        let s = "abc只def";
        // 切片 s[3..5] 是非法的 (只占3,4,5)
        // safe_slice 应该 floor 调整
        assert_eq!(safe_slice(s, 3, 5), ""); 
        assert_eq!(safe_slice_from(s, 4), "只def");
    }
}
