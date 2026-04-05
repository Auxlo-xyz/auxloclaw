//! Skill System - Markdown-based skills with progressive disclosure
//! 
//! Compatible with agentskills.io specification
//! 
//! Key Features:
//! - Progressive disclosure (4-tier loading)
//! - Natural language skill installation
//! - Self-improvement loop
//! - Conditional activation based on available tools
//! - External skill directories

use anyhow::{anyhow, bail, Result};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use walkdir::WalkDir;
use tracing::{info, warn};

/// Skill metadata (frontmatter)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMeta {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub compatibility: Option<String>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    #[serde(default)]
    pub allowed_tools: Option<String>,
    // Hermes-specific extensions
    #[serde(default)]
    pub platforms: Option<Vec<String>>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default, rename = "requires_tools")]
    pub requires_tools: Option<Vec<String>>,
    #[serde(default, rename = "fallback_for_tools")]
    pub fallback_for_tools: Option<Vec<String>>,
    #[serde(default, rename = "requires_toolsets")]
    pub requires_toolsets: Option<Vec<String>>,
    #[serde(default, rename = "fallback_for_toolsets")]
    pub fallback_for_toolsets: Option<Vec<String>>,
}

/// Skill with full content
#[derive(Debug, Clone)]
pub struct Skill {
    pub meta: SkillMeta,
    pub body: String,
    pub path: PathBuf,
    pub is_external: bool,
}

impl Skill {
    /// Parse a SKILL.md file content
    pub fn parse(content: &str) -> Result<Self> {
        // Extract frontmatter
        if !content.starts_with("---") {
            bail!("Skill must start with YAML frontmatter");
        }
        
        let end_idx = content[3..].find("---")
            .ok_or_else(|| anyhow::anyhow!("Frontmatter not closed"))?;
        
        let frontmatter = &content[3..end_idx + 3];
        let body = &content[end_idx + 6..];
        
        let meta: SkillMeta = serde_yaml::from_str(frontmatter)?;
        
        Ok(Self {
            meta,
            body: body.to_string(),
            path: PathBuf::new(), // Will be set by caller if needed
            is_external: false,
        })
    }
    
    /// Parse from file path
    pub fn from_file(path: &std::path::Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let mut skill = Self::parse(&content)?;
        skill.path = path.to_path_buf();
        Ok(skill)
    }
    
    /// Get skill name
    pub fn name(&self) -> &str {
        &self.meta.name
    }
    
    /// Get skill description
    pub fn description(&self) -> &str {
        &self.meta.description
    }
}

/// Skill index entry (Tier 0/1)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillIndexEntry {
    pub name: String,
    pub description: String,
    pub category: Option<String>,
}

/// Skill Registry with progressive disclosure
pub struct SkillRegistry {
    /// All skills (name -> skill)
    skills: DashMap<String, Skill>,
    /// Tier 0 index (always loaded in system prompt)
    tier0_index: RwLock<String>,
    /// Skill directories to scan
    skill_dirs: Vec<PathBuf>,
    /// Available tools (for conditional activation)
    available_tools: RwLock<HashSet<String>>,
    /// Available toolsets (for conditional activation)
    available_toolsets: RwLock<HashSet<String>>,
    /// Current platform
    platform: String,
}

impl SkillRegistry {
    pub fn new(primary_dir: PathBuf) -> Self {
        Self {
            skills: DashMap::new(),
            tier0_index: RwLock::new(String::new()),
            skill_dirs: vec![primary_dir],
            available_tools: RwLock::new(HashSet::new()),
            available_toolsets: RwLock::new(HashSet::new()),
            platform: Self::detect_platform(),
        }
    }

    fn detect_platform() -> String {
        if cfg!(target_os = "macos") {
            "macos".to_string()
        } else if cfg!(target_os = "linux") {
            "linux".to_string()
        } else if cfg!(target_os = "windows") {
            "windows".to_string()
        } else {
            "unknown".to_string()
        }
    }

    /// Add external skill directory (read-only)
    pub fn add_external_dir(&mut self, dir: PathBuf) {
        self.skill_dirs.push(dir);
    }

    /// Scan all skill directories and load metadata
    pub async fn scan(&self) -> Result<usize> {
        let mut count = 0;
        
        for (idx, dir) in self.skill_dirs.iter().enumerate() {
            let is_external = idx > 0;
            let expanded = Self::expand_path(dir);
            
            if !expanded.exists() {
                info!("Skill directory does not exist: {:?}", expanded);
                continue;
            }

            for entry in WalkDir::new(&expanded)
                .follow_links(true)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                if entry.file_name() == "SKILL.md" {
                    if let Ok(skill) = self.parse_skill(entry.path(), is_external).await {
                        // Check platform compatibility
                        if !self.is_compatible(&skill) {
                            continue;
                        }
                        
                        // Check conditional activation
                        if !self.should_show(&skill) {
                            continue;
                        }
                        
                        self.skills.insert(skill.meta.name.clone(), skill);
                        count += 1;
                    }
                }
            }
        }

        self.rebuild_tier0_index();
        info!("Loaded {} skills", count);
        Ok(count)
    }

    fn expand_path(path: &Path) -> PathBuf {
        let path_str = path.to_string_lossy();
        if path_str.starts_with('~') {
            if let Some(home) = std::env::var("HOME").ok() {
                PathBuf::from(path_str.replacen('~', &home, 1))
            } else {
                path.to_path_buf()
            }
        } else {
            path.to_path_buf()
        }
    }

    async fn parse_skill(&self, path: &Path, is_external: bool) -> Result<Skill> {
        let content = fs::read_to_string(path).await?;
        
        // Parse frontmatter
        let (frontmatter, body) = Self::extract_frontmatter(&content)?;
        let meta: SkillMeta = serde_yaml::from_str(&frontmatter)
            .map_err(|e| anyhow!("Failed to parse frontmatter: {}", e))?;

        Ok(Skill {
            meta,
            body: body.to_string(),
            path: path.to_path_buf(),
            is_external,
        })
    }

    fn extract_frontmatter(content: &str) -> Result<(String, &str)> {
        if !content.starts_with("---\n") {
            return Err(anyhow!("No frontmatter found"));
        }

        let end = content[4..].find("\n---\n")
            .ok_or_else(|| anyhow!("Frontmatter not terminated"))?;
        
        let frontmatter = content[4..end + 4].to_string();
        let body = &content[end + 9..]; // Skip "---\n---\n"
        
        Ok((frontmatter, body))
    }

    fn is_compatible(&self, skill: &Skill) -> bool {
        if let Some(platforms) = &skill.meta.platforms {
            platforms.contains(&self.platform)
        } else {
            true
        }
    }

    fn should_show(&self, skill: &Skill) -> bool {
        let tools = self.available_tools.read();
        let toolsets = self.available_toolsets.read();

        // Check requires_tools
        if let Some(required) = &skill.meta.requires_tools {
            if !required.iter().all(|t| tools.contains(t)) {
                return false;
            }
        }

        // Check requires_toolsets
        if let Some(required) = &skill.meta.requires_toolsets {
            if !required.iter().all(|t| toolsets.contains(t)) {
                return false;
            }
        }

        // Check fallback_for_tools (show only if tool NOT available)
        if let Some(fallback) = &skill.meta.fallback_for_tools {
            if fallback.iter().any(|t| tools.contains(t)) {
                return false;
            }
        }

        // Check fallback_for_toolsets (show only if toolset NOT available)
        if let Some(fallback) = &skill.meta.fallback_for_toolsets {
            if fallback.iter().any(|t| toolsets.contains(t)) {
                return false;
            }
        }

        true
    }

    fn rebuild_tier0_index(&self) {
        let mut index = String::from("Available skills:\n");
        
        for entry in self.skills.iter() {
            let skill = entry.value();
            index.push_str(&format!("- {}: {}\n", 
                skill.meta.name, 
                skill.meta.description.lines().next().unwrap_or("")
            ));
        }

        *self.tier0_index.write() = index;
    }

    // === Progressive Disclosure API ===

    /// Tier 0: Get compact index for system prompt (~500 tokens)
    pub fn get_tier0_index(&self) -> String {
        self.tier0_index.read().clone()
    }

    /// Tier 1: List all skills with descriptions (~3k tokens)
    pub fn list_skills(&self) -> Vec<SkillIndexEntry> {
        self.skills
            .iter()
            .map(|s| SkillIndexEntry {
                name: s.meta.name.clone(),
                description: s.meta.description.clone(),
                category: s.meta.category.clone(),
            })
            .collect()
    }

    /// Tier 2: Get full skill content
    pub fn view_skill(&self, name: &str) -> Option<Skill> {
        self.skills.get(name).map(|s| s.clone())
    }

    /// Tier 3: Get specific file within skill
    pub async fn get_skill_file(&self, name: &str, rel_path: &str) -> Option<String> {
        if let Some(skill) = self.skills.get(name) {
            let full_path = skill.path.parent()?.join(rel_path);
            fs::read_to_string(&full_path).await.ok()
        } else {
            None
        }
    }

    // === Skill Management (Self-Improvement) ===

    /// Create a new skill from natural language description
    pub async fn create_skill(&self, name: &str, description: &str, body: &str) -> Result<()> {
        let skill_dir = self.skill_dirs[0].join(name);
        fs::create_dir_all(&skill_dir).await?;

        let frontmatter = SkillMeta {
            name: name.to_string(),
            description: description.to_string(),
            license: None,
            compatibility: None,
            metadata: [("created_by".to_string(), "agent".to_string())]
                .into_iter()
                .collect(),
            allowed_tools: None,
            platforms: None,
            category: None,
            requires_tools: None,
            fallback_for_tools: None,
            requires_toolsets: None,
            fallback_for_toolsets: None,
        };

        let yaml = serde_yaml::to_string(&frontmatter)?;
        let content = format!("---\n{}---\n{}", yaml, body);

        let skill_path = skill_dir.join("SKILL.md");
        fs::write(&skill_path, &content).await?;

        // Reload
        let skill = self.parse_skill(&skill_path, false).await?;
        self.skills.insert(name.to_string(), skill);
        self.rebuild_tier0_index();

        info!("Created skill: {}", name);
        Ok(())
    }

    /// Update an existing skill
    pub async fn update_skill(&self, name: &str, body: &str) -> Result<()> {
        let mut skill = self.skills.get(name)
            .ok_or_else(|| anyhow!("Skill not found: {}", name))?
            .clone();

        // Don't modify external skills
        if skill.is_external {
            return Err(anyhow!("Cannot modify external skill: {}", name));
        }

        let yaml = serde_yaml::to_string(&skill.meta)?;
        let content = format!("---\n{}---\n{}", yaml, body);

        fs::write(&skill.path, &content).await?;

        skill.body = body.to_string();
        self.skills.insert(name.to_string(), skill);

        info!("Updated skill: {}", name);
        Ok(())
    }

    /// Delete a skill
    pub async fn delete_skill(&self, name: &str) -> Result<()> {
        let skill = self.skills.get(name)
            .ok_or_else(|| anyhow!("Skill not found: {}", name))?
            .clone();

        if skill.is_external {
            return Err(anyhow!("Cannot delete external skill: {}", name));
        }

        // Remove directory
        if let Some(parent) = skill.path.parent() {
            fs::remove_dir_all(parent).await?;
        }

        self.skills.remove(name);
        self.rebuild_tier0_index();

        info!("Deleted skill: {}", name);
        Ok(())
    }

    // === Tool Availability Tracking ===

    /// Register a tool as available
    pub fn register_tool(&self, name: &str) {
        self.available_tools.write().insert(name.to_string());
    }

    /// Register a toolset as available
    pub fn register_toolset(&self, name: &str) {
        self.available_toolsets.write().insert(name.to_string());
    }

    /// Re-evaluate skill visibility after tool changes
    pub fn reevaluate_visibility(&self) {
        // This would remove/add skills based on new tool availability
        // For now, we just rebuild the index
        self.rebuild_tier0_index();
    }
}

/// Skill Installer - installs skills from registry or creates from description
pub struct SkillInstaller {
    registry: Arc<SkillRegistry>,
    http_client: reqwest::Client,
}

impl SkillInstaller {
    pub fn new(registry: Arc<SkillRegistry>) -> Self {
        Self {
            registry,
            http_client: reqwest::Client::new(),
        }
    }

    /// Install skill from natural language prompt
    /// This is the key feature Hermes has - autonomous skill creation
    pub async fn install_from_prompt(&self, prompt: &str) -> Result<String> {
        // 1. Check if skill exists by name
        // 2. Check registry for matching skill
        // 3. Otherwise, create a new skill from the description

        // Extract skill name from prompt (simplified)
        let skill_name = self.extract_skill_name(prompt);

        // Check if already exists
        if self.registry.view_skill(&skill_name).is_some() {
            return Ok(format!("Skill '{}' already installed", skill_name));
        }

        // Create new skill
        let description = prompt.to_string();
        let body = self.generate_skill_body(prompt).await?;

        self.registry.create_skill(&skill_name, &description, &body).await?;

        Ok(format!("Created skill: {}", skill_name))
    }

    fn extract_skill_name(&self, prompt: &str) -> String {
        // Simplified extraction - in production, use LLM
        let words: Vec<&str> = prompt.split_whitespace().take(3).collect();
        words.join("-").to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect()
    }

    async fn generate_skill_body(&self, prompt: &str) -> Result<String> {
        // In production, call LLM to generate skill body
        // For now, return a template
        Ok(format!(
            r#"# Auto-generated Skill

## Description
{}

## Instructions
This skill was automatically generated based on the user's request.
Follow these instructions to accomplish the task:

1. Understand the user's goal
2. Break down into steps
3. Execute each step using available tools
4. Report results

## Tools Used
- file_read: Read files
- file_write: Write files  
- execute: Run shell commands
"#,
            prompt
        ))
    }

    /// Install from HermesHub (skills registry)
    pub async fn install_from_hub(&self, skill_name: &str, hub_url: &str) -> Result<()> {
        // Fetch skill from registry
        let url = format!("{}/skills/{}.tar.gz", hub_url, skill_name);
        let response = self.http_client.get(&url).send().await?;
        
        if !response.status().is_success() {
            return Err(anyhow!("Skill not found in hub: {}", skill_name));
        }

        // Extract to skill directory
        let bytes = response.bytes().await?;
        // ... extract tar.gz ...

        self.registry.scan().await?;
        Ok(())
    }
}

/// Learning Loop - improves skills based on experience
pub struct LearningLoop {
    registry: Arc<SkillRegistry>,
    experiences: RwLock<Vec<Experience>>,
}

#[derive(Debug, Clone)]
struct Experience {
    skill_name: String,
    task: String,
    success: bool,
    feedback: String,
    timestamp: chrono::DateTime<chrono::Utc>,
}

impl LearningLoop {
    pub fn new(registry: Arc<SkillRegistry>) -> Self {
        Self {
            registry,
            experiences: RwLock::new(Vec::new()),
        }
    }

    /// Record an experience with a skill
    pub fn record(&self, skill_name: &str, task: &str, success: bool, feedback: &str) {
        let exp = Experience {
            skill_name: skill_name.to_string(),
            task: task.to_string(),
            success,
            feedback: feedback.to_string(),
            timestamp: chrono::Utc::now(),
        };
        self.experiences.write().push(exp);
    }

    /// Improve skills based on collected experiences
    pub async fn improve(&self) -> Result<Vec<String>> {
        let mut improved = Vec::new();
        let experiences = self.experiences.read().clone();

        // Group by skill
        let mut by_skill: HashMap<String, Vec<&Experience>> = HashMap::new();
        for exp in &experiences {
            by_skill.entry(exp.skill_name.clone())
                .or_default()
                .push(exp);
        }

        for (skill_name, exps) in by_skill {
            // Calculate success rate
            let successes = exps.iter().filter(|e| e.success).count();
            let rate = successes as f32 / exps.len() as f32;

            if rate < 0.7 && exps.len() >= 3 {
                // Skill needs improvement
                let feedback: Vec<&str> = exps.iter()
                    .filter(|e| !e.success)
                    .map(|e| e.feedback.as_str())
                    .collect();

                let improvement = self.suggest_improvement(&skill_name, &feedback).await?;
                
                if let Some(skill) = self.registry.view_skill(&skill_name) {
                    let improved_body = format!("{}\n\n## Improvements\n{}", skill.body, improvement);
                    self.registry.update_skill(&skill_name, &improved_body).await?;
                    improved.push(skill_name);
                }
            }
        }

        Ok(improved)
    }

    async fn suggest_improvement(&self, skill_name: &str, failures: &[&str]) -> Result<String> {
        // In production, use LLM to analyze failures and suggest improvements
        Ok(format!(
            "Based on {} failures, consider adding:\n- Better error handling\n- More specific instructions\n- Edge case coverage",
            failures.len()
        ))
    }
}