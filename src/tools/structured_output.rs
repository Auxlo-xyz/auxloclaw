//! Structured output tool — lets the agent return JSON, CSV, files, images,
//! videos, or arbitrary structured data that channels and APIs can consume
//! programmatically.

use crate::orchestrator::{Tool, ToolResult};
use serde_json::json;
use std::path::Path;

pub struct StructuredOutputTool;

#[async_trait::async_trait]
impl Tool for StructuredOutputTool {
    fn name(&self) -> &str { "output" }

    fn description(&self) -> &str {
        "Return structured data to the user or consuming system. Supports JSON, CSV, file path, image, \
         video, or arbitrary typed content. Channels handle each format appropriately (e.g. Telegram \
         sends files as documents, API returns JSON). Use this instead of printing large data inline."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "format": {
                    "type": "string",
                    "enum": ["json", "csv", "markdown", "file", "image", "video"],
                    "description": "The output format"
                },
                "content": {
                    "description": "The output content. For json: a JSON object/array. For csv/markdown: a string. For file/image/video: the absolute path to the file on disk."
                },
                "filename": {
                    "type": "string",
                    "description": "Optional filename for downloads (e.g. \"report.csv\", \"chart.png\"). Channels use this when sending as a file attachment."
                }
            },
            "required": ["format", "content"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let format = args["format"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing required field: format"))?;
        let content = args["content"].clone();
        let filename = args["filename"].as_str().map(|s| s.to_string());

        // Validate file paths exist for file-based formats
        if matches!(format, "file" | "image" | "video") {
            let path_str = content
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("for format '{}', content must be a file path string", format))?;
            if !Path::new(path_str).exists() {
                return Ok(ToolResult {
                    tool_name: "output".into(),
                    success: false,
                    output: json!({ "error": format!("file not found: {}", path_str) }),
                    error: Some(format!("File not found: {}", path_str)),
                    duration_ms: 0,
                });
            }
        }

        let result_json = json!({
            "__structured_output__": true,
            "format": format,
            "content": content,
            "filename": filename,
        });

        Ok(ToolResult {
            tool_name: "output".into(),
            success: true,
            output: result_json,
            error: None,
            duration_ms: 0,
        })
    }
}
