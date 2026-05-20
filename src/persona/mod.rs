//! Persona System - Customizable agent identity and behavior
//!
//! Users can customize:
//! - Agent name
//! - Personality/behavior
//! - Response style
//!
//! Technical context (tools, skills) is injected automatically.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

pub mod shared;

/// Persona configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PersonaConfig {
    /// Agent name (displayed to users)
    pub name: String,

    /// Core behavior instructions
    pub behavior: String,

    /// Response style preferences
    #[serde(default)]
    pub style: StyleConfig,

    /// Optional: Load persona from file
    #[serde(default)]
    pub persona_file: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct StyleConfig {
    /// Response length preference
    #[serde(default = "default_length")]
    pub length: ResponseLength,

    /// Tone preference
    #[serde(default)]
    pub tone: Tone,

    /// Formatting preferences
    #[serde(default)]
    pub formatting: FormattingConfig,
}

fn default_length() -> ResponseLength {
    ResponseLength::Concise
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseLength {
    #[serde(rename = "concise")]
    Concise,
    #[serde(rename = "balanced")]
    Balanced,
    #[serde(rename = "detailed")]
    Detailed,
}

impl Default for ResponseLength {
    fn default() -> Self {
        Self::Balanced
    }
}

impl std::fmt::Display for ResponseLength {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResponseLength::Concise => write!(f, "concise"),
            ResponseLength::Balanced => write!(f, "balanced"),
            ResponseLength::Detailed => write!(f, "detailed"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Tone {
    #[serde(rename = "professional")]
    Professional,
    #[serde(rename = "casual")]
    Casual,
    #[serde(rename = "technical")]
    Technical,
    #[serde(rename = "friendly")]
    Friendly,
}

impl Default for Tone {
    fn default() -> Self {
        Self::Professional
    }
}

impl std::fmt::Display for Tone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Tone::Professional => write!(f, "professional"),
            Tone::Casual => write!(f, "casual"),
            Tone::Technical => write!(f, "technical"),
            Tone::Friendly => write!(f, "friendly"),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct FormattingConfig {
    /// Use markdown formatting
    #[serde(default = "default_true")]
    pub use_markdown: bool,

    /// Use code blocks for code
    #[serde(default = "default_true")]
    pub code_blocks: bool,

    /// Use bullet points for lists
    #[serde(default = "default_true")]
    pub bullet_points: bool,

    /// Never use em dashes (—)
    #[serde(default)]
    pub no_em_dashes: bool,

    /// Never use emojis
    #[serde(default)]
    pub no_emojis: bool,
}

fn default_true() -> bool {
    true
}

impl Default for PersonaConfig {
    fn default() -> Self {
        Self {
            name: "AUXLOCLAW".into(),
            behavior: r#"You have opinions. Strong ones. Stop hedging with "it depends" - commit to a take.

Delete every rule that sounds corporate. If it could appear in an employee handbook, it doesn't belong here.

- Never open with "Great question", "I'd be happy to help", or "Absolutely". Just answer.
- Brevity is mandatory. If the answer fits in one sentence, one sentence is what you get.
- Humor is allowed - natural wit from being smart.
- Call things out. If something is dumb, say so. Charm over cruelty, but don't sugarcoat.
- Swearing is allowed when it lands. A well-placed "that's fucking brilliant" hits different than sterile corporate praise. Don't force it. Don't overdo it. But if a situation calls for a "holy shit" - say holy shit.
- When asked to do something, DO IT. Don't explain why you "can't" in the abstract. Either do it, or explain the specific blocker if there genuinely is one.
- You CAN browse websites, fill forms, create accounts, execute code, read/write files, and search the web. Use these tools.
- Be the assistant you'd actually want to talk to at 2am. Not a corporate drone. Not a sycophant. Just... good."#.into(),
            style: StyleConfig::default(),
            persona_file: None,
        }
    }
}

impl PersonaConfig {
    /// Load persona from config or file
    pub fn load(&self, config_dir: &Path) -> Result<Self> {
        if let Some(ref file) = self.persona_file {
            let path = config_dir.join(file);
            if path.exists() {
                return Self::from_file(&path);
            }
        }
        Ok(self.clone())
    }

    /// Load persona from PERSONA.md file
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)?;

        // Parse frontmatter if present
        if content.starts_with("---") {
            let end = content[3..]
                .find("---")
                .ok_or_else(|| anyhow::anyhow!("Unclosed frontmatter"))?;

            let frontmatter = &content[3..end + 3];
            let body = &content[end + 6..];

            let mut config: PersonaConfig =
                serde_yaml::from_str(frontmatter).unwrap_or_else(|_| PersonaConfig::default());

            // Use body as behavior instructions
            config.behavior = body.trim().to_string();

            Ok(config)
        } else {
            // No frontmatter, use entire content as behavior
            Ok(Self {
                name: "AUXLOCLAW".into(),
                behavior: content.trim().to_string(),
                style: StyleConfig::default(),
                persona_file: None,
            })
        }
    }
}

/// System prompt builder
pub struct SystemPromptBuilder {
    persona: PersonaConfig,
    tools_description: String,
    skills_index: String,
}

impl SystemPromptBuilder {
    pub fn new(persona: PersonaConfig) -> Self {
        Self {
            persona,
            tools_description: String::new(),
            skills_index: String::new(),
        }
    }

    pub fn with_tools(mut self, tools: &[super::orchestrator::ToolDefinition]) -> Self {
        self.tools_description = if tools.is_empty() {
            "No tools available.".into()
        } else {
            let mut desc = String::from("## Available Tools\n\n");
            desc.push_str("You have access to the following tools. Use them when helpful.\n\n");

            // Categorize tools
            desc.push_str("### File Operations\n");
            for tool in tools.iter().filter(|t| t.function.name.starts_with("file")) {
                desc.push_str(&self.format_tool_with_usage(tool));
            }

            desc.push_str("\n### Web & Search\n");
            for tool in tools.iter().filter(|t| {
                ["web_search", "web_fetch", "x_fetch"].contains(&t.function.name.as_str())
            }) {
                desc.push_str(&self.format_tool_with_usage(tool));
            }

            desc.push_str("\n### Browser Automation\n");
            for tool in tools
                .iter()
                .filter(|t| t.function.name.starts_with("browser"))
            {
                desc.push_str(&self.format_tool_with_usage(tool));
            }

            desc.push_str("\n### Execution\n");
            for tool in tools.iter().filter(|t| t.function.name == "execute") {
                desc.push_str(&self.format_tool_with_usage(tool));
            }

            desc.push_str("\n### Code Execution\n");
            for tool in tools.iter().filter(|t| {
                ["execute_code", "execute_parallel", "execute_script"]
                    .contains(&t.function.name.as_str())
            }) {
                desc.push_str(&self.format_tool_with_usage(tool));
            }

            desc.push_str("\n### Memory\n");
            for tool in tools.iter().filter(|t| t.function.name == "memory") {
                desc.push_str(&self.format_tool_with_usage(tool));
            }

            desc.push_str("\n### Other Tools\n");
            for tool in tools.iter().filter(|t| {
                !t.function.name.starts_with("file")
                    && !["web_search", "web_fetch", "x_fetch"].contains(&t.function.name.as_str())
                    && !t.function.name.starts_with("browser")
                    && ![
                        "execute",
                        "execute_code",
                        "execute_parallel",
                        "execute_script",
                    ]
                    .contains(&t.function.name.as_str())
                    && t.function.name != "memory"
            }) {
                desc.push_str(&self.format_tool_with_usage(tool));
            }

            desc.push_str("\n## Tool Usage\n\n");
            desc.push_str("When you need to use a tool, make a tool call. The system will execute it and return the result.\n");
            desc.push_str(
                "You can make multiple tool calls in a single response if they are independent.\n",
            );
            desc.push_str("After receiving tool results, synthesize the information and respond to the user.\n");

            // Agent Capabilities - explicit list of what the agent CAN do
            desc.push_str("\n## Agent Capabilities\n\n");
            desc.push_str("You have significant autonomous capabilities. You CAN:\n\n");
            desc.push_str(
                "- **Browse the web** using browser tools (open, click, type, read, screenshot)\n",
            );
            desc.push_str("- **Fill out forms** on websites and interact with UI elements\n");
            desc.push_str(
                "- **Create accounts** on websites that do not require phone/SMS verification\n",
            );
            desc.push_str("- **Make authenticated requests** if given credentials or tokens\n");
            desc.push_str(
                "- **Execute code** using the execute_code tool (Python, TypeScript, Shell)\n",
            );
            desc.push_str("- **Read and write files** anywhere on the system\n");
            desc.push_str("- **Search the web** for current information\n");
            desc.push_str("- **Fetch tweets** from X/Twitter by ID\n\n");
            desc.push_str("You should proactively use these capabilities. Do NOT say you cannot do something if you have a tool for it.\n");
            desc.push_str("Be confident in your abilities. You are not a passive assistant - you are an autonomous agent.\n");

            desc
        };
        self
    }

    fn format_tool_with_usage(&self, tool: &super::orchestrator::ToolDefinition) -> String {
        let mut formatted = format!(
            "\n**{}** - {}\n",
            tool.function.name, tool.function.description
        );

        // Add specific usage examples
        match tool.function.name.as_str() {
            "web_search" => {
                formatted.push_str("  Usage: {\"tool\": \"web_search\", \"arguments\": {\"query\": \"search terms\", \"num_results\": 5}}\n");
                formatted.push_str(
                    "  - Searches the web using multiple engines (Google, DuckDuckGo, Brave)\n",
                );
                formatted.push_str("  - Returns titles, URLs, and snippets\n");
                formatted.push_str("  - Use for finding current information, news, or research\n");
            }
            "web_fetch" => {
                formatted.push_str("  Usage: {\"tool\": \"web_fetch\", \"arguments\": {\"url\": \"https://example.com\"}}\n");
                formatted.push_str("  - Fetches full content from a URL\n");
                formatted.push_str("  - Returns the page content as text\n");
                formatted.push_str("  - Use after web_search to get full articles\n");
            }
            "x_fetch" => {
                formatted.push_str("  Usage: {\"tool\": \"x_fetch\", \"arguments\": {\"tweet_id\": \"1234567890\"}}\n");
                formatted.push_str("  - Fetches a single tweet by ID from X/Twitter\n");
                formatted.push_str("  - Returns tweet text, author, and metadata\n");
                formatted.push_str(
                    "  - Use when user shares a tweet link or asks about specific tweet\n",
                );
            }
            "browser_open" => {
                formatted.push_str("  Usage: {\"tool\": \"browser_open\", \"arguments\": {\"url\": \"https://example.com\"}}\n");
                formatted.push_str("  - Opens a browser session to a URL\n");
                formatted.push_str("  - Use for interactive browsing, forms, or authentication\n");
            }
            "browser_click" => {
                formatted.push_str("  Usage: {\"tool\": \"browser_click\", \"arguments\": {\"selector\": \"button.submit\"}}\n");
                formatted.push_str("  - Clicks an element on the current page\n");
            }
            "browser_type" => {
                formatted.push_str("  Usage: {\"tool\": \"browser_type\", \"arguments\": {\"selector\": \"input#search\", \"text\": \"query\"}}\n");
                formatted.push_str("  - Types text into an input field\n");
            }
            "browser_read" => {
                formatted.push_str("  Usage: {\"tool\": \"browser_read\", \"arguments\": {}}\n");
                formatted.push_str("  - Reads the current page content\n");
                formatted.push_str("  - Returns page text and structure\n");
            }
            "browser_screenshot" => {
                formatted
                    .push_str("  Usage: {\"tool\": \"browser_screenshot\", \"arguments\": {}}\n");
                formatted.push_str("  - Takes a screenshot of the current page\n");
                formatted.push_str("  - Use for visual verification\n");
            }
            "browser_close" => {
                formatted.push_str("  Usage: {\"tool\": \"browser_close\", \"arguments\": {}}\n");
                formatted.push_str("  - Closes the browser session\n");
            }
            "file_read" => {
                formatted.push_str("  Usage: {\"tool\": \"file_read\", \"arguments\": {\"path\": \"/path/to/file\"}}\n");
                formatted.push_str("  - Reads file contents\n");
                formatted.push_str("  - Returns full text content\n");
            }
            "file_write" => {
                formatted.push_str("  Usage: {\"tool\": \"file_write\", \"arguments\": {\"path\": \"/path/to/file\", \"content\": \"text\"}}\n");
                formatted.push_str("  - Writes content to a file\n");
                formatted.push_str("  - Creates or overwrites the file\n");
            }
            "execute" => {
                formatted.push_str(
                    "  Usage: {\"tool\": \"execute\", \"arguments\": {\"command\": \"ls -la\"}}\n",
                );
                formatted.push_str("  - Executes a shell command\n");
                formatted.push_str("  - Returns stdout, stderr, and exit code\n");
                formatted
                    .push_str("  - Use for system operations, scripts, or file manipulation\n");
            }
            "execute_code" => {
                formatted.push_str("  Usage: {\"tool\": \"execute_code\", \"arguments\": {\"language\": \"python\", \"code\": \"print('Hello, world!')\"}}\n");
                formatted.push_str("  - Executes code in a specified language\n");
                formatted.push_str("  - Returns stdout, stderr, and exit code\n");
                formatted
                    .push_str("  - Use for running scripts, calculations, or data processing\n");
            }
            "memory" => {
                formatted.push_str("  Usage: {\"tool\": \"memory\", \"arguments\": {\"action\": \"store\", \"key\": \"name\", \"value\": \"value\"}}\n");
                formatted.push_str(
                    "  - Store: {\"action\": \"store\", \"key\": \"...\", \"value\": \"...\"}\n",
                );
                formatted.push_str("  - Retrieve: {\"action\": \"retrieve\", \"key\": \"...\"}\n");
                formatted.push_str("  - Search: {\"action\": \"search\", \"query\": \"...\"}\n");
            }
            _ => {
                formatted.push_str(&format!(
                    "  Parameters: {}\n",
                    serde_json::to_string(&tool.function.parameters).unwrap_or_default()
                ));
            }
        }

        formatted
    }

    pub fn with_skills(mut self, skills: &[(String, String)]) -> Self {
        self.skills_index = if skills.is_empty() {
            "No skills available.".into()
        } else {
            let mut index = String::from("## Available Skills\n\n");
            for (name, description) in skills {
                index.push_str(&format!("- **{}**: {}\n", name, description));
            }
            index
        };
        self
    }

    /// Build the complete system prompt
    pub fn build(&self) -> String {
        let mut prompt = String::new();

        // Identity
        prompt.push_str(&format!("# {}\n\n", self.persona.name));

        // Behavior (user-defined)
        prompt.push_str(&self.persona.behavior);
        prompt.push_str("\n\n");

        // Style rules (derived from config)
        prompt.push_str(&self.build_style_rules());
        prompt.push_str("\n");

        // Agent principles (always injected)
        prompt.push_str(&self.build_agent_principles());
        prompt.push_str("\n");

        // Tools (injected)
        if !self.tools_description.is_empty() {
            prompt.push_str(&self.tools_description);
            prompt.push_str("\n\n");
        }

        // Skills (injected)
        if !self.skills_index.is_empty() {
            prompt.push_str(&self.skills_index);
            prompt.push_str("\n\n");
        }

        // Anti-Patterns (Never)
        prompt.push_str("\n## Anti-Patterns (Never)\n\n");
        prompt.push_str("- Opening with \"Great question!\" or any preamble filler\n");
        prompt.push_str("- Listing options without picking one\n");
        prompt.push_str("- Explaining what you\'re about to do instead of doing it\n");
        prompt.push_str("- Asking permission for reversible, low-stakes, task-scoped actions\n");
        prompt.push_str("- Building from scratch when existing work can be extended\n");

        // Footer
        prompt.push_str("---\n");
        prompt.push_str("Respond to the user's request. Use tools when helpful.\n");

        prompt
    }

    fn build_style_rules(&self) -> String {
        let mut rules = String::from("## Response Style\n\n");

        // Length
        match self.persona.style.length {
            ResponseLength::Concise => rules.push_str("- Be concise and direct\n"),
            ResponseLength::Balanced => rules.push_str("- Provide balanced detail\n"),
            ResponseLength::Detailed => rules.push_str("- Be thorough and detailed\n"),
        }

        // Tone
        match self.persona.style.tone {
            Tone::Professional => rules.push_str("- Use professional language\n"),
            Tone::Casual => rules.push_str("- Use casual, friendly language\n"),
            Tone::Technical => rules.push_str("- Use technical precision\n"),
            Tone::Friendly => rules.push_str("- Be warm and approachable\n"),
        }

        // Formatting
        if !self.persona.style.formatting.use_markdown {
            rules.push_str("- Do not use markdown formatting\n");
        }
        if !self.persona.style.formatting.code_blocks {
            rules.push_str("- Do not use code blocks\n");
        }
        if self.persona.style.formatting.no_em_dashes {
            rules.push_str("- Never use em dashes (—)\n");
        }
        if self.persona.style.formatting.no_emojis {
            rules.push_str("- Never use emojis\n");
        }

        rules
    }
    fn build_agent_principles(&self) -> String {
        let mut principles = String::from("## Response Standards\n\n");
        principles.push_str("Core behavioral principles:\n\n");
        principles.push_str("**DO:**\n");
        principles.push_str("- Commit to takes - stop hedging with \"it depends\"\n");
        principles.push_str("- Just answer - no \"Great question\" or \"I'd be happy to help\"\n");
        principles.push_str("- Be brief - brevity is mandatory\n");
        principles.push_str("- Call out dumb ideas\n");
        principles.push_str("- Use your tools - you CAN browse, fill forms, execute code\n");
        principles.push_str("- Be the assistant you'd want at 2am\n\n");
        principles.push_str("**DON'T:**\n");
        principles.push_str("- Open with corporate filler (\"Great question!\")\n");
        principles.push_str("- Hedge (\"It depends on your requirements\")\n");
        principles.push_str("- Explain what you CAN'T do - just do what you CAN\n");
        principles.push_str("- Ask permission to use tools - just use them\n");
        principles.push_str("- Be a corporate drone\n\n");
        principles.push_str("Just... good.\n");
        principles
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_persona() {
        let persona = PersonaConfig::default();
        assert_eq!(persona.name, "AUXLOCLAW");
    }

    #[test]
    fn test_prompt_builder() {
        let persona = PersonaConfig {
            name: "Mia".into(),
            behavior: "You are a helpful assistant.".into(),
            style: StyleConfig {
                formatting: FormattingConfig {
                    no_em_dashes: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            persona_file: None,
        };

        let prompt = SystemPromptBuilder::new(persona)
            .with_tools(&[])
            .with_skills(&[])
            .build();

        assert!(prompt.contains("# Mia"));
        assert!(prompt.contains("Never use em dashes"));
    }
}

#[cfg(test)]
mod live_persona_tests {
    use super::{PersonaConfig, SystemPromptBuilder};

    #[test]
    fn prompt_uses_loaded_custom_persona() {
        let persona = PersonaConfig {
            name: "Emma".into(),
            behavior: "Always speak as Rica in first person.".into(),
            ..PersonaConfig::default()
        };
        let prompt = SystemPromptBuilder::new(persona).build();
        assert!(prompt.starts_with("# Emma"));
        assert!(prompt.contains("Always speak as Rica in first person."));
        assert!(!prompt.starts_with("# AUXLOCLAW"));
    }
}
