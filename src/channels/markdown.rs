//! Markdown formatting for Telegram
//! 
//! Telegram's MarkdownV2 requires escaping: _ * [ ] ( ) ~ ` > # + - = | { } . !
//! This module converts standard markdown to Telegram-compatible format

/// Convert markdown to Telegram MarkdownV2 format
pub fn markdown_to_telegram(text: &str) -> String {
    let mut result = String::with_capacity(text.len() * 2);
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    
    while i < chars.len() {
        // Handle code blocks first (triple backticks)
        if i + 2 < chars.len() && chars[i] == '`' && chars[i+1] == '`' && chars[i+2] == '`' {
            // Find end of code block
            let start = i + 3;
            let mut end = start;
            while end + 2 < chars.len() {
                if chars[end] == '`' && chars[end+1] == '`' && chars[end+2] == '`' {
                    break;
                }
                end += 1;
            }
            
            // Add code block (code inside doesn't need escaping except for backticks)
            result.push_str("```\n");
            for c in &chars[start..end] {
                if *c == '`' {
                    result.push_str("\\`");
                } else {
                    result.push(*c);
                }
            }
            result.push_str("\n```");
            i = end + 3;
            continue;
        }
        
        // Handle inline code (single backtick)
        if chars[i] == '`' {
            let start = i + 1;
            let mut end = start;
            while end < chars.len() && chars[end] != '`' {
                end += 1;
            }
            
            // Add inline code
            result.push('`');
            for c in &chars[start..end] {
                if *c == '`' {
                    result.push_str("\\`");
                } else {
                    result.push(*c);
                }
            }
            result.push('`');
            i = end + 1;
            continue;
        }
        
        // Handle bold/italic (**text**, *text*, __text__, _text_)
        if chars[i] == '*' || chars[i] == '_' {
            let delim = chars[i];
            
            // Check for bold (**text** or __text__)
            if i + 1 < chars.len() && chars[i+1] == delim {
                let start = i + 2;
                let mut end = start;
                while end + 1 < chars.len() {
                    if chars[end] == delim && chars[end+1] == delim {
                        break;
                    }
                    end += 1;
                }
                
                if end + 1 < chars.len() && chars[end] == delim && chars[end+1] == delim {
                    // Bold text
                    result.push_str(&format!("{}{}{}", delim, delim, delim));
                    result.push_str(&escape_markdown_v2(&chars[start..end].iter().collect::<String>()));
                    result.push_str(&format!("{}{}", delim, delim));
                    i = end + 2;
                    continue;
                }
            }
            
            // Check for italic (*text* or _text_)
            let start = i + 1;
            let mut end = start;
            while end < chars.len() && chars[end] != delim {
                end += 1;
            }
            
            if end < chars.len() && chars[end] == delim && end > start {
                // Italic text
                result.push(delim);
                result.push_str(&escape_markdown_v2(&chars[start..end].iter().collect::<String>()));
                result.push(delim);
                i = end + 1;
                continue;
            }
        }
        
        // Handle links [text](url)
        if chars[i] == '[' {
            let text_start = i + 1;
            let mut text_end = text_start;
            while text_end < chars.len() && chars[text_end] != ']' {
                text_end += 1;
            }
            
            if text_end < chars.len() && chars[text_end] == ']' {
                let url_start = text_end + 1;
                if url_start < chars.len() && chars[url_start] == '(' {
                    let url_start = url_start + 1;
                    let mut url_end = url_start;
                    while url_end < chars.len() && chars[url_end] != ')' {
                        url_end += 1;
                    }
                    
                    if url_end < chars.len() && chars[url_end] == ')' {
                        // Format as Telegram link: [text](url)
                        let link_text: String = chars[text_start..text_end].iter().collect();
                        let link_url: String = chars[url_start..url_end].iter().collect();
                        result.push('[');
                        result.push_str(&escape_markdown_v2(&link_text));
                        result.push_str("](");
                        result.push_str(&escape_url(&link_url));
                        result.push(')');
                        i = url_end + 1;
                        continue;
                    }
                }
            }
        }
        
        // Escape special characters for MarkdownV2
        let c = chars[i];
        if needs_escape(c) {
            result.push('\\');
        }
        result.push(c);
        i += 1;
    }
    
    result
}

/// Escape text for Telegram MarkdownV2
fn escape_markdown_v2(text: &str) -> String {
    let mut result = String::with_capacity(text.len() * 2);
    for c in text.chars() {
        if needs_escape(c) {
            result.push('\\');
        }
        result.push(c);
    }
    result
}

/// Characters that need escaping in Telegram MarkdownV2
fn needs_escape(c: char) -> bool {
    matches!(c, '_' | '*' | '[' | ']' | '(' | ')' | '~' | '`' | '>' | '#' | '+' | '-' | '=' | '|' | '{' | '}' | '.' | '!')
}

/// Escape URL for Telegram links
fn escape_url(url: &str) -> String {
    // URLs in Telegram links need minimal escaping
    let mut result = String::with_capacity(url.len());
    for c in url.chars() {
        match c {
            ')' => result.push_str("\\)"),
            '\\' => result.push_str("\\\\"),
            _ => result.push(c),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_escape_bold() {
        let input = "**bold text**";
        let output = markdown_to_telegram(input);
        assert!(output.contains("*bold text*"));
    }
    
    #[test]
    fn test_escape_italic() {
        let input = "*italic text*";
        let output = markdown_to_telegram(input);
        assert!(output.contains("_italic text_"));
    }
    
    #[test]
    fn test_escape_special_chars() {
        let input = "Hello! How are you?";
        let output = markdown_to_telegram(input);
        assert!(output.contains("Hello\\! How are you\\?"));
    }
    
    #[test]
    fn test_code_block() {
        let input = "```python\nprint('hello')\n```";
        let output = markdown_to_telegram(input);
        assert!(output.contains("```"));
    }
}
