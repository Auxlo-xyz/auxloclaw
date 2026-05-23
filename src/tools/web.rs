//! Web Tools - search, browser automation, X/Twitter

use async_trait::async_trait;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;
use regex::Regex;

use crate::orchestrator::{Tool, ToolResult};

// =====================================================
// WEB SEARCH TOOL (DuckDuckGo HTML - no API key needed)
// =====================================================

pub struct WebSearchTool;

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str { "web_search" }
    
    fn description(&self) -> &str { 
        "Search the web using DuckDuckGo. Returns JSON results with titles, URLs, and snippets. No API key required."
    }
    
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Max results (default: 10)",
                    "default": 10
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let query = args["query"].as_str()
            .ok_or_else(|| anyhow!("Missing query parameter"))?;
        
        let max_results = args.get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as usize;
        
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15")
            .timeout(std::time::Duration::from_secs(15))
            .build()?;
        
        search_ddg(&client, query, max_results).await
    }
}

fn decode_ddg_url(href: &str) -> String {
    // DDG returns URLs directly now (no uddg redirect)
    let url = href.replace("&amp;", "&");
    // Decode percent-encoded characters
    let mut result = String::with_capacity(url.len());
    let bytes = url.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(
                std::str::from_utf8(&bytes[i+1..i+3]).unwrap_or("XX"), 16
            ) {
                result.push(byte as char);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

fn strip_html_tags(s: &str) -> String {
    let re = Regex::new(r"<[^>]+>").unwrap();
    re.replace_all(s, "").to_string()
}

fn decode_html_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&#x2F;", "/")
        .replace("&#47;", "/")
}

async fn search_ddg(
    client: &reqwest::Client,
    query: &str,
    max_results: usize,
) -> Result<ToolResult> {
    // POST to DuckDuckGo HTML endpoint (HTTP/2)
    // This is what the ddgs Python library does - it bypasses CAPTCHAs
    let params = [
        ("q", query),
        ("b", ""),
        ("l", "us-en"),
    ];
    
    let resp = client.post("https://html.duckduckgo.com/html/")
        .form(&params)
        .send().await?;
    
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Ok(ToolResult {
            tool_name: "web_search".into(),
            success: false,
            output: serde_json::json!({
                "error": format!("DuckDuckGo returned HTTP {}: {}", status, &body[..body.len().min(200)])
            }),
            error: Some(format!("HTTP {}", status)),
            duration_ms: 0,
        });
    }
    
    let html = resp.text().await?;
    
    // Check for CAPTCHA challenge
    if html.contains("anomaly-modal") {
        return Ok(ToolResult {
            tool_name: "web_search".into(),
            success: false,
            output: serde_json::json!({
                "error": "DuckDuckGo CAPTCHA triggered. Try again later or use a proxy.",
                "provider": "duckduckgo"
            }),
            error: Some("CAPTCHA".into()),
            duration_ms: 0,
        });
    }
    
    // Parse results from HTML
    // Structure: <a class="result__a" href="URL">TITLE</a>
    //            <a class="result__snippet" href="URL">SNIPPET</a>
    let title_re = Regex::new(r#"<a[^>]*class="result__a"[^>]*href="([^"]*)"[^>]*>(.*?)</a>"#)?;
    let snippet_re = Regex::new(r#"<a[^>]*class="result__snippet"[^>]*href="([^"]*)"[^>]*>(.*?)</a>"#)?;
    
    let titles: Vec<(String, String)> = title_re.captures_iter(&html)
        .map(|cap| {
            let url = decode_ddg_url(cap.get(1).map(|m| m.as_str()).unwrap_or(""));
            let title = decode_html_entities(&strip_html_tags(cap.get(2).map(|m| m.as_str()).unwrap_or("")));
            (url, title)
        })
        .collect();
    
    let snippets: Vec<String> = snippet_re.captures_iter(&html)
        .map(|cap| {
            decode_html_entities(&strip_html_tags(cap.get(2).map(|m| m.as_str()).unwrap_or("")))
        })
        .collect();
    
    let mut results: Vec<serde_json::Value> = Vec::new();
    for (i, (url, title)) in titles.iter().enumerate() {
        if results.len() >= max_results {
            break;
        }
        if url.is_empty() || url.starts_with("javascript:") || url.contains("duckduckgo.com/y.js") {
            continue;
        }
        let snippet = snippets.get(i).cloned().unwrap_or_default();
        results.push(serde_json::json!({
            "title": title,
            "url": url,
            "snippet": snippet
        }));
    }
    
    Ok(ToolResult {
        tool_name: "web_search".into(),
        success: true,
        output: serde_json::json!({
            "query": query,
            "provider": "duckduckgo",
            "total": results.len(),
            "results": results
        }),
        error: None,
        duration_ms: 0,
    })
}

// =====================================================
// BROWSER TOOLS (uses agent-browser CLI)
// =====================================================

pub struct BrowserOpenTool;

#[async_trait]
impl Tool for BrowserOpenTool {
    fn name(&self) -> &str { "browser_open" }
    fn description(&self) -> &str { "Open a URL in the browser for subsequent automation" }
    
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "URL to open" }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let url = args["url"].as_str()
            .ok_or_else(|| anyhow!("Missing url parameter"))?;
        
        let output = Command::new("agent-browser")
            .arg("open")
            .arg(url)
            .output()
            .map_err(|e| anyhow!("Failed to run agent-browser: {}", e))?;
        
        Ok(ToolResult {
            tool_name: self.name().into(),
            success: output.status.success(),
            output: serde_json::json!({
                "url": url,
                "success": output.status.success(),
                "message": String::from_utf8_lossy(&output.stdout).to_string()
            }),
            error: if output.status.success() { None } else { 
                Some(String::from_utf8_lossy(&output.stderr).to_string()) 
            },
            duration_ms: 0,
        })
    }
}

pub struct BrowserSnapshotTool;

#[async_trait]
impl Tool for BrowserSnapshotTool {
    fn name(&self) -> &str { "browser_snapshot" }
    fn description(&self) -> &str { "Get accessibility tree snapshot of current page with element refs (@e1, @e2, etc.) for interaction" }
    
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "interactive": { 
                    "type": "boolean", 
                    "description": "Only show interactive elements (default: true)",
                    "default": true
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let interactive = args.get("interactive")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        
        let mut cmd = Command::new("agent-browser");
        cmd.arg("snapshot");
        if interactive {
            cmd.arg("-i");
        }
        
        let output = cmd.output()
            .map_err(|e| anyhow!("Failed to run agent-browser: {}", e))?;
        
        let snapshot = String::from_utf8_lossy(&output.stdout).to_string();
        
        Ok(ToolResult {
            tool_name: self.name().into(),
            success: output.status.success(),
            output: serde_json::json!({
                "snapshot": snapshot
            }),
            error: None,
            duration_ms: 0,
        })
    }
}

pub struct BrowserClickTool;

#[async_trait]
impl Tool for BrowserClickTool {
    fn name(&self) -> &str { "browser_click" }
    fn description(&self) -> &str { "Click an element by selector or @ref from snapshot" }
    
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "selector": { "type": "string", "description": "CSS selector or @ref (e.g., @e5)" }
            },
            "required": ["selector"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let selector = args["selector"].as_str()
            .ok_or_else(|| anyhow!("Missing selector parameter"))?;
        
        let output = Command::new("agent-browser")
            .arg("click")
            .arg(selector)
            .output()
            .map_err(|e| anyhow!("Failed to run agent-browser: {}", e))?;
        
        Ok(ToolResult {
            tool_name: self.name().into(),
            success: output.status.success(),
            output: serde_json::json!({
                "clicked": selector,
                "success": output.status.success()
            }),
            error: if output.status.success() { None } else { 
                Some(String::from_utf8_lossy(&output.stderr).to_string()) 
            },
            duration_ms: 0,
        })
    }
}

pub struct BrowserFillTool;

#[async_trait]
impl Tool for BrowserFillTool {
    fn name(&self) -> &str { "browser_fill" }
    fn description(&self) -> &str { "Fill text into an input field (clears first)" }
    
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "selector": { "type": "string", "description": "CSS selector or @ref" },
                "text": { "type": "string", "description": "Text to fill" }
            },
            "required": ["selector", "text"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let selector = args["selector"].as_str()
            .ok_or_else(|| anyhow!("Missing selector parameter"))?;
        let text = args["text"].as_str()
            .ok_or_else(|| anyhow!("Missing text parameter"))?;
        
        let output = Command::new("agent-browser")
            .arg("fill")
            .arg(selector)
            .arg(text)
            .output()
            .map_err(|e| anyhow!("Failed to run agent-browser: {}", e))?;
        
        Ok(ToolResult {
            tool_name: self.name().into(),
            success: output.status.success(),
            output: serde_json::json!({
                "filled": selector,
                "text_length": text.len(),
                "success": output.status.success()
            }),
            error: if output.status.success() { None } else { 
                Some(String::from_utf8_lossy(&output.stderr).to_string()) 
            },
            duration_ms: 0,
        })
    }
}

pub struct BrowserScreenshotTool;

#[async_trait]
impl Tool for BrowserScreenshotTool {
    fn name(&self) -> &str { "browser_screenshot" }
    fn description(&self) -> &str { "Take a screenshot of the current page" }
    
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { 
                    "type": "string", 
                    "description": "Path to save screenshot (default: /tmp/screenshot.png)",
                    "default": "/tmp/screenshot.png"
                },
                "full_page": {
                    "type": "boolean",
                    "description": "Capture full page (default: false)",
                    "default": false
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let path = args.get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("/tmp/screenshot.png");
        
        let full_page = args.get("full_page")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        
        let mut cmd = Command::new("agent-browser");
        cmd.arg("screenshot").arg(path);
        if full_page {
            cmd.arg("--full-page");
        }
        
        let output = cmd.output()
            .map_err(|e| anyhow!("Failed to run agent-browser: {}", e))?;
        
        Ok(ToolResult {
            tool_name: self.name().into(),
            success: output.status.success(),
            output: serde_json::json!({
                "path": path,
                "saved": output.status.success()
            }),
            error: if output.status.success() { None } else { 
                Some(String::from_utf8_lossy(&output.stderr).to_string()) 
            },
            duration_ms: 0,
        })
    }
}

pub struct BrowserGetTool;

#[async_trait]
impl Tool for BrowserGetTool {
    fn name(&self) -> &str { "browser_get" }
    fn description(&self) -> &str { "Get content from page: text, html, url, title" }
    
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "what": { 
                    "type": "string", 
                    "description": "What to get: text, html, url, title",
                    "enum": ["text", "html", "url", "title"]
                }
            },
            "required": ["what"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let what = args["what"].as_str()
            .ok_or_else(|| anyhow!("Missing what parameter"))?;
        
        let output = Command::new("agent-browser")
            .arg("get")
            .arg(what)
            .output()
            .map_err(|e| anyhow!("Failed to run agent-browser: {}", e))?;
        
        let content = String::from_utf8_lossy(&output.stdout).to_string();
        
        Ok(ToolResult {
            tool_name: self.name().into(),
            success: output.status.success(),
            output: serde_json::json!({
                what: content
            }),
            error: if output.status.success() { None } else { 
                Some(String::from_utf8_lossy(&output.stderr).to_string()) 
            },
            duration_ms: 0,
        })
    }
}

pub struct BrowserCloseTool;

#[async_trait]
impl Tool for BrowserCloseTool {
    fn name(&self) -> &str { "browser_close" }
    fn description(&self) -> &str { "Close the browser session" }
    
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({ "type": "object", "properties": {} })
    }

    async fn execute(&self, _args: serde_json::Value) -> Result<ToolResult> {
        let output = Command::new("agent-browser")
            .arg("close")
            .output()
            .map_err(|e| anyhow!("Failed to run agent-browser: {}", e))?;
        
        Ok(ToolResult {
            tool_name: self.name().into(),
            success: output.status.success(),
            output: serde_json::json!({ "closed": true }),
            error: None,
            duration_ms: 0,
        })
    }
}

// =====================================================
// X/TWITTER TOOL (uses vxtwitter API)
// =====================================================

pub struct XFetchTool;

#[async_trait]
impl Tool for XFetchTool {
    fn name(&self) -> &str { "x_fetch" }
    fn description(&self) -> &str { "Fetch tweet/user info from X/Twitter via vxtwitter API (no auth required)" }
    
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "type": {
                    "type": "string",
                    "description": "What to fetch: tweet or user",
                    "enum": ["tweet", "user"]
                },
                "id": {
                    "type": "string",
                    "description": "Tweet ID or username (e.g., 123456789 or elonmusk)"
                }
            },
            "required": ["type", "id"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let fetch_type = args["type"].as_str()
            .ok_or_else(|| anyhow!("Missing type parameter"))?;
        let id = args["id"].as_str()
            .ok_or_else(|| anyhow!("Missing id parameter"))?;
        
        let url = match fetch_type {
            "tweet" => format!("https://api.vxtwitter.com/Twitter/status/{}", id),
            "user" => format!("https://api.vxtwitter.com/{}", id),
            _ => return Err(anyhow!("Invalid type: must be 'tweet' or 'user'"))
        };
        
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (compatible; AUXLOCLAW/0.1)")
            .build()?;
        
        let resp = client.get(&url).send().await?;
        
        if !resp.status().is_success() {
            return Ok(ToolResult {
                tool_name: self.name().into(),
                success: false,
                output: serde_json::json!({
                    "error": format!("HTTP {}", resp.status()),
                    "url": url
                }),
                error: Some(format!("HTTP {}", resp.status())),
                duration_ms: 0,
            });
        }
        
        let data: serde_json::Value = resp.json().await?;
        
        Ok(ToolResult {
            tool_name: self.name().into(),
            success: true,
            output: data,
            error: None,
            duration_ms: 0,
        })
    }
}
