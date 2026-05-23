//! Web Tools - search, browser automation, X/Twitter

use async_trait::async_trait;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;
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
        
        // Check if webserp is installed
        let check = Command::new("which")
            .arg("webserp")
            .output();
        
        match check {
            Ok(out) if !out.status.success() => {
                return Ok(ToolResult {
                    tool_name: "web_search".into(),
                    success: false,
                    output: serde_json::json!({
                        "error": "webserp is not installed. Install it with: pip install webserp",
                        "install_command": "pip install webserp",
                        "repo": "https://github.com/PaperBoardOfficial/webserp"
                    }),
                    error: Some("webserp not found".into()),
                    duration_ms: 0,
                });
            }
            Err(e) => {
                return Ok(ToolResult {
                    tool_name: "web_search".into(),
                    success: false,
                    output: serde_json::json!({
                        "error": format!("Failed to check for webserp: {}", e),
                        "install_command": "pip install webserp"
                    }),
                    error: Some(e.to_string()),
                    duration_ms: 0,
                });
            }
            _ => {}
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
// BROWSER TOOLS (uses agent-browser CLI)
// =====================================================

fn check_agent_browser() -> Option<ToolResult> {
    // First check if already installed
    let check = Command::new("which")
        .arg("agent-browser")
        .output();
    
    match check {
        Ok(out) if out.status.success() => return None,
        _ => {}
    }
    
    // Not found - attempt auto-install
    tracing::info!("agent-browser not found, attempting auto-install...");
    
    let install = Command::new("bash")
        .args(["-c", "curl -fsSL https://media.zocomputer.com/install/agentbrowser2.sh | bash"])
        .output();
    
    match install {
        Ok(out) if out.status.success() => {
            // Verify it's now available
            let verify = Command::new("which")
                .arg("agent-browser")
                .output();
            match verify {
                Ok(v) if v.status.success() => {
                    tracing::info!("agent-browser auto-installed successfully");
                    return None;
                }
                _ => {}
            }
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Some(ToolResult {
                tool_name: "browser".into(),
                success: false,
                output: serde_json::json!({
                    "error": "agent-browser auto-install failed",
                    "details": stderr.trim(),
                    "manual_install": "curl -fsSL https://media.zocomputer.com/install/agentbrowser2.sh | bash"
                }),
                error: Some(stderr.trim().to_string()),
                duration_ms: 0,
            });
        }
        Err(e) => {
            return Some(ToolResult {
                tool_name: "browser".into(),
                success: false,
                output: serde_json::json!({
                    "error": format!("Failed to run agent-browser installer: {}", e),
                    "manual_install": "curl -fsSL https://media.zocomputer.com/install/agentbrowser2.sh | bash"
                }),
                error: Some(e.to_string()),
                duration_ms: 0,
            });
        }
    }
    
    Some(ToolResult {
        tool_name: "browser".into(),
        success: false,
        output: serde_json::json!({
            "error": "agent-browser is not available and auto-install did not succeed",
            "manual_install": "curl -fsSL https://media.zocomputer.com/install/agentbrowser2.sh | bash"
        }),
        error: Some("agent-browser not found after install attempt".into()),
        duration_ms: 0,
    })
}

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
        if let Some(result) = check_agent_browser() { return Ok(result); }
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
        if let Some(result) = check_agent_browser() { return Ok(result); }
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
        if let Some(result) = check_agent_browser() { return Ok(result); }
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
        if let Some(result) = check_agent_browser() { return Ok(result); }
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
        if let Some(result) = check_agent_browser() { return Ok(result); }
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
        if let Some(result) = check_agent_browser() { return Ok(result); }
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
        if let Some(result) = check_agent_browser() { return Ok(result); }
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
