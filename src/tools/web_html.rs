use std::borrow::Cow;

#[derive(Debug, Clone)]
pub struct SearchItem {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// 安全切片辅助函数：如果索引不在字符边界，自动向前寻找最近的合法边界
/// 防止 "byte index is not a char boundary" Panic
fn safe_slice(s: &str, start: usize, end: usize) -> &str {
    let len = s.len();
    let start = s.floor_char_boundary(start.min(len));
    let end = s.floor_char_boundary(end.min(len));
    
    if start >= end {
        return "";
    }
    
    // 双重保险，使用 get 避免任何可能的越界
    s.get(start..end).unwrap_or("")
}

/// 安全切片（RangeFrom）：对应 s[start..]
fn safe_slice_from(s: &str, start: usize) -> &str {
    let len = s.len();
    let start = s.floor_char_boundary(start.min(len));
    s.get(start..).unwrap_or("")
}

fn strip_block(mut html: String, tag: &str) -> String {
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    
    // 限制循环次数防止死循环，并在每次修改后重置搜索位置
    // 因为 replace_range 会改变字符串长度，索引会失效
    // 但简单的做法是每次从头找，或者仔细维护 offset。
    // 原逻辑维护了 offset 是高效的，但需要确保边界安全。
    let mut search_start = 0;
    
    loop {
        // 使用 safe_slice_from 确保搜索子串是安全的，虽然 find_case_insensitive 内部处理了 from
        let Some(start) = find_case_insensitive(&html, &open, search_start) else {
            break;
        };
        
        let Some(end) = find_case_insensitive(&html, &close, start) else {
            // 没有闭合标签，直接截断到开始处
            html.truncate(start);
            break;
        };
        
        let remove_to = end + close.len();
        
        // 关键：确保 remove_to 是合法的字符边界
        if !html.is_char_boundary(remove_to) {
            // 如果计算出的结束位置非法，尝试向后找最近的边界
            // 这里通常是因为 close.len() 是 ASCII 没问题，但防御性处理
            search_start = start + 1; 
            continue; 
        }

        // replace_range 要求 range 的 start 和 end 必须是 char boundary
        // start 来自 find (safe), remove_to 检查过
        html.replace_range(start..remove_to, " ");
        
        // 下次搜索从替换位置开始（现在是一个空格）
        search_start = start;
    }
    html
}

fn find_case_insensitive(haystack: &str, needle: &str, from: usize) -> Option<usize> {
    if from >= haystack.len() {
        return None;
    }

    // 确保起始搜索位置是合法的字符边界
    let from = haystack.floor_char_boundary(from);
    
    // 安全获取子串
    let h_slice = haystack.get(from..)?; 
    let h = h_slice.to_ascii_lowercase();
    let n = needle.to_ascii_lowercase();
    
    // 注意：这里假设 to_ascii_lowercase 不会改变字符串的字节长度
    // 对于绝大多数 UTF-8 场景这是成立的，但如果索引对不齐，下面的 idx 可能会有问题
    // 所以调用方拿到结果后，再次切片前一定要检查
    h.find(&n).map(|idx| from + idx)
}

pub fn decode_html_entities(input: &str) -> Cow<'_, str> {
    if !input.contains('&') {
        return Cow::Borrowed(input);
    }

    let mut out = input.to_string();
    // 扩展常见实体
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
    
    // 危险点 1：基于 lower_tag 的索引切片 tag
    // 修复：使用 safe_slice_from
    let raw_start = idx + needle.len();
    let raw = safe_slice_from(tag, raw_start);
    let raw = raw.trim_start();
    
    if raw.is_empty() {
        return None;
    }

    let first = raw.chars().next()?;
    if first == '"' || first == '\'' {
        let quote = first;
        // 危险点 2：&raw[1..]
        // 修复：1 是 quote 的长度（ASCII），通常安全，但用 get 更稳妥
        let rest = raw.get(1..)?;
        let end = rest.find(quote)?;
        // 危险点 3：切片到 end
        return Some(safe_slice(rest, 0, end).to_string());
    }

    let end = raw
        .find(|c: char| c.is_whitespace() || c == '>')
        .unwrap_or(raw.len());
    
    // 危险点 4
    Some(safe_slice(raw, 0, end).to_string())
}

fn extract_snippet_near(html: &str, from: usize) -> String {
    // 原始代码这里已经做了 floor_char_boundary，很好
    let from = html.floor_char_boundary(from.min(html.len()));
    let window_end = html.floor_char_boundary(from.saturating_add(4000).min(html.len()));
    
    // 使用 safe_slice 替代 &html[...]
    let segment = safe_slice(html, from, window_end);
    
    let Some(class_idx) = find_case_insensitive(segment, "result__snippet", 0) else {
        return String::new();
    };

    // safe_slice 替代 segment[..class_idx]
    let pre_segment = safe_slice(segment, 0, class_idx);
    let tag_start = pre_segment.rfind('<').unwrap_or(0);
    
    // safe_slice_from 替代 segment[tag_start..]
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

    // 危险点：tag_end + 1 可能是乱码位置（虽然 > 是 1 字节）
    // 修复：使用 safe_slice
    let inner = safe_slice(segment, tag_end + 1, close_rel);
    html_to_text(inner)
}

pub fn extract_ddg_results(html: &str, max_results: usize) -> Vec<SearchItem> {
    let mut results = Vec::new();
    let mut pos = 0usize;

    while results.len() < max_results {
        let Some(a_start) = find_case_insensitive(html, "<a", pos) else {
            break;
        };
        
        // 安全切片查找 >
        let search_area = safe_slice_from(html, a_start);
        let Some(a_tag_end_rel) = search_area.find('>') else {
            break;
        };
        let a_tag_end = a_start + a_tag_end_rel;
        
        // 获取完整的 a 标签字符串用于提取属性
        let a_tag = safe_slice(html, a_start, a_tag_end + 1);

        let class = extract_attr(a_tag, "class").unwrap_or_default();
        if !class.split_whitespace().any(|c| c == "result__a") {
            pos = a_tag_end + 1;
            continue;
        }

        let Some(close_rel) = find_case_insensitive(html, "</a>", a_tag_end + 1) else {
            break;
        };

        let href = extract_attr(a_tag, "href")
            .map(|h| decode_html_entities(&h).into_owned())
            .unwrap_or_default();
            
        // 危险点：切片 title_html
        let title_html = safe_slice(html, a_tag_end + 1, close_rel);
        let title = html_to_text(title_html);
        
        // 提取摘要
        let snippet = extract_snippet_near(html, close_rel + 4);

        if !href.is_empty() && !title.is_empty() {
            results.push(SearchItem {
                title,
                url: href,
                snippet,
            });
        }

        pos = close_rel + 4;
    }

    results
}

pub fn extract_primary_html(html: &str) -> &str {
    let candidates = ["main", "article", "body"];
    for tag in candidates {
        let open = format!("<{tag}");
        let close = format!("</{tag}>");
        if let Some(start) = find_case_insensitive(html, &open, 0) {
            // 安全切片查找 >
            let search_area = safe_slice_from(html, start);
            if let Some(open_end_rel) = search_area.find('>') {
                let content_start = start + open_end_rel + 1;
                
                // 这里 content_start 依赖于 +1，虽然 > 是 ASCII，但最好防御一下
                // find_case_insensitive 内部会处理 content_start 的边界
                if let Some(end) = find_case_insensitive(html, &close, content_start) {
                    // 最终切片返回
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
    fn test_find_case_insensitive_non_char_boundary_input() {
        let s = "abc只def";
        // byte 4 is inside the multi-byte '只'
        // find_case_insensitive 现在会内部 floor 到 index 3
        // 然后 slice "只def" 找 "def" -> index 3 + 3 = 6
        assert_eq!(find_case_insensitive(s, "def", 4), Some(6));
    }
    
    #[test]
    fn test_safe_slice_panic_prevention() {
        let s = "abc只def"; // '只' 占用 3 bytes (indices 3,4,5)
        // 尝试切片 s[3..5] (非法)
        // safe_slice 应该把它变成 s[3..3] 或者 s[3..6] 取决于逻辑，这里逻辑是 floor
        // start=3(OK), end=5(Inside->Floor to 3). Result: ""
        assert_eq!(safe_slice(s, 3, 5), "");
        
        // 尝试切片 s[4..]
        // floor(4) -> 3. "只def"
        assert_eq!(safe_slice_from(s, 4), "只def");
    }
}
