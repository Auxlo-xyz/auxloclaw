//! Web Tools - search, browser automation, X/Twitter

use async_trait::async_trait;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::process::{Command, Stdio};
use regex::Regex;

use crate::orchestrator::{Tool, ToolResult};

// =====================================================
// WEB SEARCH TOOL (webserp CLI - multi-engine, no API key)
// =====================================================

pub struct WebSearchTool;

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str { "web_search" }
    
    fn description(&self) -> &str { 
        "Search the web using multiple engines (Google, DuckDuckGo, Brave, Yahoo, Mojeek, Startpage, Presearch) in parallel. No API key required. Returns JSON results with titles, URLs, and snippets."
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
                    "description": "Max results per engine (default: 10)",
                    "default": 10
                },
                "engines": {
                    "type": "string",
                    "description": "Comma-separated engine list (default: all). Options: google, duckduckgo, brave, yahoo, mojeek, startpage, presearch"
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
            .unwrap_or(10);
        
        let engines = args.get("engines")
            .and_then(|v| v.as_str());
        
        // Auto-install webserp if missing
        let has_webserp = Command::new("which")
            .arg("webserp")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        
        if !has_webserp {
            tracing::info!("webserp not found, auto-installing via pip...");
            let install = Command::new("pip")
                .args(["install", "webserp"])
                .output();
            match install {
                Ok(o) if o.status.success() => {
                    tracing::info!("webserp installed successfully");
                }
                Ok(o) => {
                    let stderr = String::from_utf8_lossy(&o.stderr);
                    return Ok(ToolResult {
                        tool_name: "web_search".into(),
                        success: false,
                        output: serde_json::json!({
                            "error": format!("Failed to install webserp: {}", stderr.trim()),
                            "fix": "Run manually: pip install webserp"
                        }),
                        error: Some(stderr.trim().to_string()),
                        duration_ms: 0,
                    });
                }
                Err(e) => {
                    return Ok(ToolResult {
                        tool_name: "web_search".into(),
                        success: false,
                        output: serde_json::json!({
                            "error": format!("pip not available: {}", e),
                            "fix": "Install pip, then run: pip install webserp"
                        }),
                        error: Some(e.to_string()),
                        duration_ms: 0,
                    });
                }
            }
        }
        
        // Build command
        let mut cmd = Command::new("webserp");
        cmd.arg(query);
        cmd.arg("--max-results").arg(max_results.to_string());
        if let Some(eng) = engines {
            cmd.arg("--engines").arg(eng);
        }
        
        let output = cmd.output()
            .map_err(|e| anyhow!("Failed to run webserp: {}", e))?;
        
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Ok(ToolResult {
                tool_name: "web_search".into(),
                success: false,
                output: serde_json::json!({
                    "error": format!("webserp failed: {}", stderr.trim()),
                    "query": query
                }),
                error: Some(stderr.trim().to_string()),
                duration_ms: 0,
            });
        }
        
        let stdout = String::from_utf8_lossy(&output.stdout);
        
        // Parse webserp JSON output
        let parsed: serde_json::Value = serde_json::from_str(&stdout)
            .unwrap_or_else(|_| serde_json::json!({
                "raw": stdout.to_string()
            }));
        
        let results = parsed.get("results")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        
        let unresponsive = parsed.get("unresponsive_engines")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        
        Ok(ToolResult {
            tool_name: "web_search".into(),
            success: true,
            output: serde_json::json!({
                "query": query,
                "provider": "webserp",
                "total": results.len(),
                "unresponsive_engines": unresponsive,
                "results": results
            }),
            error: None,
            duration_ms: 0,
        })
    }
}

// =====================================================
// BROWSER TOOLS (uses lightpanda-cdp - 20MB memory, no Chrome)
// =====================================================

fn find_cdp_script() -> String {
    // Check common locations
    let candidates = [
        "/usr/local/lib/auxloclaw/lightpanda-cdp",
        "/opt/auxloclaw/lightpanda-cdp",
    ];
    for path in &candidates {
        if std::path::Path::new(path).exists() {
            return path.to_string();
        }
    }
    // Fall back to PATH
    "lightpanda-cdp".to_string()
}

fn ensure_lightpanda() -> Result<()> {
    // Check lightpanda binary
    let has_lp = Command::new("which")
        .arg("lightpanda")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !has_lp {
        tracing::info!("lightpanda not found, auto-installing...");
        let install_ok = Command::new("sh")
            .args(["-c", "LP_ARCH=$(uname -m | sed 's/arm64/aarch64/') && LP_OS=$(uname -s | tr 'A-Z' 'a-z') && curl -fsSL -o /usr/local/bin/lightpanda \"https://github.com/lightpanda-io/browser/releases/download/nightly/lightpanda-${LP_ARCH}-${LP_OS}\" && chmod +x /usr/local/bin/lightpanda"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !install_ok {
            return Err(anyhow!("lightpanda auto-install failed. Install manually:\ncurl -fsSL -o /usr/local/bin/lightpanda https://github.com/lightpanda-io/browser/releases/download/nightly/lightpanda-$(uname -m | sed 's/arm64/aarch64/')-$(uname -s | tr 'A-Z' 'a-z') && chmod +x /usr/local/bin/lightpanda"));
        }
        tracing::info!("lightpanda installed successfully");
    }

    // Check websockets Python package
    let has_ws = Command::new("python3")
        .args(["-c", "import websockets"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !has_ws {
        tracing::info!("websockets not found, auto-installing...");
        let install = Command::new("pip")
            .args(["install", "websockets", "-q"])
            .output();
        match install {
            Ok(o) if o.status.success() => {
                tracing::info!("websockets installed successfully");
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                anyhow::bail!("websockets install failed: {}", stderr.trim());
            }
            Err(e) => {
                anyhow::bail!("websockets install error: {}", e);
            }
        }
    }

    Ok(())
}

fn run_cdp(args: &[&str]) -> Result<(bool, serde_json::Value)> {
    let script = find_cdp_script();
    let output = Command::new("python3")
        .arg(&script)
        .args(args)
        .output()
        .map_err(|e| anyhow!("Failed to run lightpanda-cdp: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|_| serde_json::json!({"raw": stdout.to_string()}));

    Ok((output.status.success(), parsed))
}

pub struct BrowserOpenTool;

#[async_trait]
impl Tool for BrowserOpenTool {
    fn name(&self) -> &str { "browser_open" }
    fn description(&self) -> &str {
        "Open a URL in the Lightpanda browser (20MB memory) for subsequent automation (click, fill, screenshot)"
    }
    
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
        if let Err(e) = ensure_lightpanda() {
            return Ok(ToolResult {
                tool_name: "browser".into(),
                success: false,
                output: serde_json::json!({"error": e.to_string()}),
                error: Some(e.to_string()),
                duration_ms: 0,
            });
        }
        let url = args["url"].as_str()
            .ok_or_else(|| anyhow!("Missing url parameter"))?;
        let start = std::time::Instant::now();
        let (ok, data) = run_cdp(&["open", url])?;
        let duration_ms = start.elapsed().as_millis() as u64;
        Ok(ToolResult {
            tool_name: self.name().into(),
            success: ok && data.get("ok").and_then(|v| v.as_bool()).unwrap_or(false),
            output: data,
            error: if ok { None } else { Some("browser_open failed".into()) },
            duration_ms,
        })
    }
}

pub struct BrowserSnapshotTool;

#[async_trait]
impl Tool for BrowserSnapshotTool {
    fn name(&self) -> &str { "browser_snapshot" }
    fn description(&self) -> &str {
        "Get accessibility tree snapshot of current page with element refs (@e1, @e2, etc.) for interaction via click or fill"
    }
    
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _args: serde_json::Value) -> Result<ToolResult> {
        if let Err(e) = ensure_lightpanda() {
            return Ok(ToolResult {
                tool_name: "browser".into(),
                success: false,
                output: serde_json::json!({"error": e.to_string()}),
                error: Some(e.to_string()),
                duration_ms: 0,
            });
        }
        let start = std::time::Instant::now();
        let (ok, data) = run_cdp(&["snapshot"])?;
        let duration_ms = start.elapsed().as_millis() as u64;
        Ok(ToolResult {
            tool_name: self.name().into(),
            success: ok && data.get("ok").and_then(|v| v.as_bool()).unwrap_or(false),
            output: data,
            error: if ok { None } else { Some("browser_snapshot failed".into()) },
            duration_ms,
        })
    }
}

pub struct BrowserClickTool;

#[async_trait]
impl Tool for BrowserClickTool {
    fn name(&self) -> &str { "browser_click" }
    fn description(&self) -> &str {
        "Click an element by CSS selector on the current page. Use browser_snapshot first to discover elements."
    }
    
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "selector": { "type": "string", "description": "CSS selector (e.g., 'button', 'a.link', 'input[type=submit]')" }
            },
            "required": ["selector"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        if let Err(e) = ensure_lightpanda() {
            return Ok(ToolResult {
                tool_name: "browser".into(),
                success: false,
                output: serde_json::json!({"error": e.to_string()}),
                error: Some(e.to_string()),
                duration_ms: 0,
            });
        }
        let selector = args["selector"].as_str()
            .ok_or_else(|| anyhow!("Missing selector parameter"))?;
        let start = std::time::Instant::now();
        let (ok, data) = run_cdp(&["click", selector])?;
        let duration_ms = start.elapsed().as_millis() as u64;
        Ok(ToolResult {
            tool_name: self.name().into(),
            success: ok && data.get("ok").and_then(|v| v.as_bool()).unwrap_or(false),
            output: data,
            error: if ok { None } else { Some("browser_click failed".into()) },
            duration_ms,
        })
    }
}

pub struct BrowserFillTool;

#[async_trait]
impl Tool for BrowserFillTool {
    fn name(&self) -> &str { "browser_fill" }
    fn description(&self) -> &str {
        "Fill text into an input field by CSS selector (clears existing value first). Triggers input/change events."
    }
    
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "selector": { "type": "string", "description": "CSS selector for the input field" },
                "text": { "type": "string", "description": "Text to fill" }
            },
            "required": ["selector", "text"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        if let Err(e) = ensure_lightpanda() {
            return Ok(ToolResult {
                tool_name: "browser".into(),
                success: false,
                output: serde_json::json!({"error": e.to_string()}),
                error: Some(e.to_string()),
                duration_ms: 0,
            });
        }
        let selector = args["selector"].as_str()
            .ok_or_else(|| anyhow!("Missing selector parameter"))?;
        let text = args["text"].as_str()
            .ok_or_else(|| anyhow!("Missing text parameter"))?;
        let start = std::time::Instant::now();
        let (ok, data) = run_cdp(&["fill", selector, text])?;
        let duration_ms = start.elapsed().as_millis() as u64;
        Ok(ToolResult {
            tool_name: self.name().into(),
            success: ok && data.get("ok").and_then(|v| v.as_bool()).unwrap_or(false),
            output: data,
            error: if ok { None } else { Some("browser_fill failed".into()) },
            duration_ms,
        })
    }
}

pub struct BrowserScreenshotTool;

#[async_trait]
impl Tool for BrowserScreenshotTool {
    fn name(&self) -> &str { "browser_screenshot" }
    fn description(&self) -> &str {
        "Take a PNG screenshot of the current page"
    }
    
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to save screenshot (default: /tmp/screenshot.png)",
                    "default": "/tmp/screenshot.png"
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        if let Err(e) = ensure_lightpanda() {
            return Ok(ToolResult {
                tool_name: "browser".into(),
                success: false,
                output: serde_json::json!({"error": e.to_string()}),
                error: Some(e.to_string()),
                duration_ms: 0,
            });
        }
        let path = args.get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("/tmp/screenshot.png");
        let start = std::time::Instant::now();
        let (ok, data) = run_cdp(&["screenshot", path])?;
        let duration_ms = start.elapsed().as_millis() as u64;
        Ok(ToolResult {
            tool_name: self.name().into(),
            success: ok && data.get("ok").and_then(|v| v.as_bool()).unwrap_or(false),
            output: data,
            error: if ok { None } else { Some("browser_screenshot failed".into()) },
            duration_ms,
        })
    }
}

pub struct BrowserGetTool;

#[async_trait]
impl Tool for BrowserGetTool {
    fn name(&self) -> &str { "browser_get" }
    fn description(&self) -> &str {
        "Get content from the current page: text, html, url, or title"
    }
    
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
        if let Err(e) = ensure_lightpanda() {
            return Ok(ToolResult {
                tool_name: "browser".into(),
                success: false,
                output: serde_json::json!({"error": e.to_string()}),
                error: Some(e.to_string()),
                duration_ms: 0,
            });
        }
        let what = args["what"].as_str()
            .ok_or_else(|| anyhow!("Missing what parameter"))?;
        let start = std::time::Instant::now();
        let (ok, data) = run_cdp(&["get", what])?;
        let duration_ms = start.elapsed().as_millis() as u64;
        Ok(ToolResult {
            tool_name: self.name().into(),
            success: ok && data.get("ok").and_then(|v| v.as_bool()).unwrap_or(false),
            output: data,
            error: if ok { None } else { Some("browser_get failed".into()) },
            duration_ms,
        })
    }
}

pub struct BrowserCloseTool;

#[async_trait]
impl Tool for BrowserCloseTool {
    fn name(&self) -> &str { "browser_close" }
    fn description(&self) -> &str {
        "Close the browser session and stop the Lightpanda server"
    }
    
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({ "type": "object", "properties": {} })
    }

    async fn execute(&self, _args: serde_json::Value) -> Result<ToolResult> {
        let start = std::time::Instant::now();
        let (ok, data) = run_cdp(&["close"])?;
        let duration_ms = start.elapsed().as_millis() as u64;
        Ok(ToolResult {
            tool_name: self.name().into(),
            success: ok,
            output: data,
            error: None,
            duration_ms,
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

// =====================================================
// WEB FETCH TOOL (lightpanda - 20MB memory, fast page reading)
// =====================================================

pub struct WebFetchTool;

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str { "web_fetch" }
    fn description(&self) -> &str {
        "Fetch a web page and return its content as markdown. Uses Lightpanda engine (20MB memory, 10x faster than Chrome). For reading page content, articles, documentation. No API key required."
    }
    
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "URL to fetch"
                },
                "wait_ms": {
                    "type": "integer",
                    "description": "Wait time in milliseconds for JS to finish (default: 5000)",
                    "default": 5000
                },
                "strip": {
                    "type": "string",
                    "description": "What to strip: js, ui, css, full (default: full for clean text)",
                    "enum": ["js", "ui", "css", "full"],
                    "default": "full"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult> {
        let url = args["url"].as_str()
            .ok_or_else(|| anyhow!("Missing url parameter"))?;
        
        let wait_ms = args.get("wait_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(5000)
            .to_string();
        
        let strip = args.get("strip")
            .and_then(|v| v.as_str())
            .unwrap_or("full");
        
        if let Err(e) = ensure_lightpanda() {
            return Ok(ToolResult {
                tool_name: "web_fetch".into(),
                success: false,
                output: serde_json::json!({"error": e.to_string()}),
                error: Some(e.to_string()),
                duration_ms: 0,
            });
        }
        
        let start = std::time::Instant::now();
        
        let output = Command::new("lightpanda")
            .arg("fetch")
            .arg(url)
            .arg("--dump")
            .arg("markdown")
            .arg("--json")
            .arg("--strip-mode")
            .arg(strip)
            .arg("--wait-ms")
            .arg(&wait_ms)
            .arg("--terminate-ms")
            .arg("15000")
            .output()
            .map_err(|e| anyhow!("Failed to run lightpanda: {}", e))?;
        
        let duration_ms = start.elapsed().as_millis() as u64;
        
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Ok(ToolResult {
                tool_name: "web_fetch".into(),
                success: false,
                output: serde_json::json!({
                    "error": format!("lightpanda fetch failed: {}", stderr.trim()),
                    "url": url
                }),
                error: Some(stderr.trim().to_string()),
                duration_ms,
            });
        }
        
        let stdout = String::from_utf8_lossy(&output.stdout);
        
        // Parse JSON output from lightpanda
        let parsed: serde_json::Value = serde_json::from_str(&stdout)
            .unwrap_or_else(|_| serde_json::json!({"raw": stdout.to_string()}));
        
        let content = parsed.get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        let status = parsed.get("http_status")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        
        Ok(ToolResult {
            tool_name: "web_fetch".into(),
            success: status >= 200 && status < 400 && !content.is_empty(),
            output: serde_json::json!({
                "url": url,
                "status": status,
                "content": content,
                "char_count": content.len(),
                "duration_ms": duration_ms
            }),
            error: None,
            duration_ms,
        })
    }
}
