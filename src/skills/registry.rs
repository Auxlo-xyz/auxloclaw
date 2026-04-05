//! Skill Registry - Auto-discovery and installation

use anyhow::{bail, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

/// Official AUXLOCLAW skill registry
pub const REGISTRY_URL: &str = "https://raw.githubusercontent.com/auxlo/skills/main/manifest.json";

/// AgentSkills.io compatible registry
pub const AGENTSKILLS_REGISTRY: &str = "https://agentskills.io/api/v1/skills";

/// Skill metadata from registry
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RegistrySkill {
    pub name: String,
    pub description: String,
    pub category: String,
    pub github_url: String,
    pub author: String,
    pub version: String,
    pub compatibility: Option<String>,
    pub tags: Vec<String>,
}

/// Skill registry manifest
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RegistryManifest {
    pub version: String,
    pub skills: Vec<RegistrySkill>,
    pub categories: HashMap<String, String>,
}

/// Skill registry client
pub struct SkillRegistry {
    client: Client,
    cache: Vec<RegistrySkill>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| Client::new()),
            cache: Vec::new(),
        }
    }

    /// View a skill from local skills directory
    pub fn view_skill(&self, name: &str) -> Option<super::Skill> {
        let skills_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("auxloclaw")
            .join("skills");

        for entry in walkdir::WalkDir::new(&skills_dir)
            .min_depth(2)
            .max_depth(3)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_name() == "SKILL.md" {
                if let Ok(content) = fs::read_to_string(entry.path()) {
                    if let Ok(skill) = super::Skill::parse(&content) {
                        if skill.name() == name {
                            return Some(skill);
                        }
                    }
                }
            }
        }
        None
    }

    /// Update a skill's body
    pub async fn update_skill(&self, name: &str, body: &str) -> Result<()> {
        let skills_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("auxloclaw")
            .join("skills");

        for entry in walkdir::WalkDir::new(&skills_dir)
            .min_depth(2)
            .max_depth(3)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_name() == "SKILL.md" {
                if let Ok(content) = fs::read_to_string(entry.path()) {
                    if let Ok(skill) = super::Skill::parse(&content) {
                        if skill.name() == name {
                            // Write updated content
                            let new_content = format!(
                                "---\nname: {}\ndescription: {}\n---\n{}",
                                skill.meta.name,
                                skill.meta.description,
                                body
                            );
                            fs::write(entry.path(), new_content)?;
                            return Ok(());
                        }
                    }
                }
            }
        }
        bail!("Skill '{}' not found", name)
    }

    /// Create a new skill
    pub async fn create_skill(&self, name: &str, description: &str, body: &str) -> Result<()> {
        let skills_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("auxloclaw")
            .join("skills");

        let skill_dir = skills_dir.join(name);
        fs::create_dir_all(&skill_dir)?;

        let content = format!(
            "---\nname: {}\ndescription: {}\n---\n{}",
            name, description, body
        );

        fs::write(skill_dir.join("SKILL.md"), content)?;
        Ok(())
    }

    /// Scan skills directory
    pub async fn scan(&mut self) -> Result<()> {
        self.cache = self.fetch_registry().await?;
        Ok(())
    }

    /// Search for skills matching a query
    pub async fn search(&mut self, query: &str) -> Result<Vec<RegistrySkill>> {
        if self.cache.is_empty() {
            self.cache = self.fetch_registry().await?;
        }

        let query_lower = query.to_lowercase();
        let results: Vec<RegistrySkill> = self.cache
            .iter()
            .filter(|s| {
                s.name.contains(&query_lower) ||
                s.description.to_lowercase().contains(&query_lower) ||
                s.tags.iter().any(|t| t.to_lowercase().contains(&query_lower))
            })
            .cloned()
            .collect();

        Ok(results)
    }

    /// Get skill by name
    pub async fn get(&mut self, name: &str) -> Result<Option<RegistrySkill>> {
        if self.cache.is_empty() {
            self.cache = self.fetch_registry().await?;
        }

        Ok(self.cache.iter().find(|s| s.name == name).cloned())
    }

    /// List all available skills
    pub async fn list(&mut self) -> Result<Vec<RegistrySkill>> {
        if self.cache.is_empty() {
            self.cache = self.fetch_registry().await?;
        }

        Ok(self.cache.clone())
    }

    /// List skills by category
    pub async fn list_by_category(&mut self, category: &str) -> Result<Vec<RegistrySkill>> {
        if self.cache.is_empty() {
            self.cache = self.fetch_registry().await?;
        }

        Ok(self.cache
            .iter()
            .filter(|s| s.category == category)
            .cloned()
            .collect())
    }

    /// Fetch registry from remote
    async fn fetch_registry(&self) -> Result<Vec<RegistrySkill>> {
        // Try official registry first
        if let Ok(response) = self.client.get(REGISTRY_URL).send().await {
            if response.status().is_success() {
                if let Ok(manifest) = response.json::<RegistryManifest>().await {
                    return Ok(manifest.skills);
                }
            }
        }

        // Fallback to built-in skill list
        Ok(self.get_builtin_skills())
    }

    /// Built-in skills that come with AUXLOCLAW
    fn get_builtin_skills(&self) -> Vec<RegistrySkill> {
        vec![
            RegistrySkill {
                name: "code-review".into(),
                description: "Perform thorough code reviews with security and quality focus".into(),
                category: "software-development".into(),
                github_url: "https://github.com/auxlo/skills/tree/main/software-development/code-review".into(),
                author: "auxlo".into(),
                version: "1.0.0".into(),
                compatibility: None,
                tags: vec!["code".into(), "review".into(), "security".into()],
            },
            RegistrySkill {
                name: "arxiv".into(),
                description: "Search and retrieve academic papers from arXiv".into(),
                category: "research".into(),
                github_url: "https://github.com/auxlo/skills/tree/main/research/arxiv".into(),
                author: "auxlo".into(),
                version: "1.0.0".into(),
                compatibility: None,
                tags: vec!["research".into(), "papers".into(), "academic".into()],
            },
            RegistrySkill {
                name: "fine-tuning-axolotl".into(),
                description: "Expert guidance for fine-tuning LLMs with Axolotl".into(),
                category: "mlops".into(),
                github_url: "https://github.com/auxlo/skills/tree/main/mlops/fine-tuning-axolotl".into(),
                author: "auxlo".into(),
                version: "1.0.0".into(),
                compatibility: None,
                tags: vec!["llm".into(), "training".into(), "fine-tuning".into()],
            },
            RegistrySkill {
                name: "test-driven-development".into(),
                description: "Enforce RED-GREEN-REFACTOR cycle with test-first approach".into(),
                category: "software-development".into(),
                github_url: "https://github.com/auxlo/skills/tree/main/software-development/tdd".into(),
                author: "auxlo".into(),
                version: "1.0.0".into(),
                compatibility: None,
                tags: vec!["testing".into(), "tdd".into(), "development".into()],
            },
            RegistrySkill {
                name: "systematic-debugging".into(),
                description: "4-phase root cause investigation methodology".into(),
                category: "software-development".into(),
                github_url: "https://github.com/auxlo/skills/tree/main/software-development/debugging".into(),
                author: "auxlo".into(),
                version: "1.0.0".into(),
                compatibility: None,
                tags: vec!["debugging".into(), "investigation".into(), "root-cause".into()],
            },
            RegistrySkill {
                name: "web-scraping".into(),
                description: "Extract structured data from websites using various tools".into(),
                category: "data".into(),
                github_url: "https://github.com/auxlo/skills/tree/main/data/web-scraping".into(),
                author: "auxlo".into(),
                version: "1.0.0".into(),
                compatibility: None,
                tags: vec!["scraping".into(), "extraction".into(), "data".into()],
            },
            RegistrySkill {
                name: "api-integration".into(),
                description: "Integrate with external APIs and services".into(),
                category: "software-development".into(),
                github_url: "https://github.com/auxlo/skills/tree/main/software-development/api".into(),
                author: "auxlo".into(),
                version: "1.0.0".into(),
                compatibility: None,
                tags: vec!["api".into(), "integration".into(), "rest".into()],
            },
            RegistrySkill {
                name: "git-workflow".into(),
                description: "Git branching strategies, commit conventions, and PR workflows".into(),
                category: "software-development".into(),
                github_url: "https://github.com/auxlo/skills/tree/main/software-development/git".into(),
                author: "auxlo".into(),
                version: "1.0.0".into(),
                compatibility: None,
                tags: vec!["git".into(), "workflow".into(), "version-control".into()],
            },
            RegistrySkill {
                name: "docker-deployment".into(),
                description: "Containerize and deploy applications with Docker".into(),
                category: "devops".into(),
                github_url: "https://github.com/auxlo/skills/tree/main/devops/docker".into(),
                author: "auxlo".into(),
                version: "1.0.0".into(),
                compatibility: None,
                tags: vec!["docker".into(), "deployment".into(), "containers".into()],
            },
            RegistrySkill {
                name: "prompt-engineering".into(),
                description: "Advanced prompt engineering techniques and patterns".into(),
                category: "ai".into(),
                github_url: "https://github.com/auxlo/skills/tree/main/ai/prompts".into(),
                author: "auxlo".into(),
                version: "1.0.0".into(),
                compatibility: None,
                tags: vec!["prompt".into(), "llm".into(), "engineering".into()],
            },
        ]
    }

    /// Install a skill from GitHub URL
    pub async fn install_from_github(&self, url: &str, skills_dir: &std::path::Path) -> Result<String> {
        // Parse GitHub URL to get repo and path
        let (owner, repo, path) = parse_github_url(url)?;
        
        // Fetch SKILL.md content
        let skill_url = format!("https://raw.githubusercontent.com/{}/{}/main/{}/SKILL.md", owner, repo, path);
        
        let response = self.client.get(&skill_url).send().await?;
        
        if !response.status().is_success() {
            bail!("Failed to fetch skill from GitHub: {}", response.status());
        }

        let content = response.text().await?;
        
        // Parse skill name from frontmatter
        let skill_name = extract_skill_name(&content)?;
        
        // Create skill directory
        let skill_dir = skills_dir.join(&skill_name);
        std::fs::create_dir_all(&skill_dir)?;
        
        // Write SKILL.md
        std::fs::write(skill_dir.join("SKILL.md"), content)?;
        
        // Try to fetch additional files (scripts, references)
        self.fetch_skill_assets(&owner, &repo, &path, &skill_dir).await?;
        
        Ok(skill_name)
    }

    async fn fetch_skill_assets(&self, owner: &str, repo: &str, path: &str, skill_dir: &std::path::Path) -> Result<()> {
        // Try to fetch scripts directory
        let scripts_url = format!("https://api.github.com/repos/{}/{}/contents/{}/scripts", owner, repo, path);
        
        if let Ok(response) = self.client.get(&scripts_url).send().await {
            if response.status().is_success() {
                if let Ok(files) = response.json::<Vec<GitHubContent>>().await {
                    std::fs::create_dir_all(skill_dir.join("scripts"))?;
                    
                    for file in files {
                        if file.r#type == "file" {
                            if let Ok(content) = self.fetch_file_content(&file.download_url).await {
                                std::fs::write(skill_dir.join("scripts").join(&file.name), content)?;
                            }
                        }
                    }
                }
            }
        }

        // Try to fetch references directory
        let refs_url = format!("https://api.github.com/repos/{}/{}/contents/{}/references", owner, repo, path);
        
        if let Ok(response) = self.client.get(&refs_url).send().await {
            if response.status().is_success() {
                if let Ok(files) = response.json::<Vec<GitHubContent>>().await {
                    std::fs::create_dir_all(skill_dir.join("references"))?;
                    
                    for file in files {
                        if file.r#type == "file" {
                            if let Ok(content) = self.fetch_file_content(&file.download_url).await {
                                std::fs::write(skill_dir.join("references").join(&file.name), content)?;
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn fetch_file_content(&self, url: &str) -> Result<String> {
        let response = self.client.get(url).send().await?;
        Ok(response.text().await?)
    }
}

#[derive(Debug, Deserialize)]
struct GitHubContent {
    name: String,
    #[serde(rename = "type")]
    r#type: String,
    download_url: String,
}

/// Parse GitHub URL into (owner, repo, path)
fn parse_github_url(url: &str) -> Result<(String, String, String)> {
    // Handle various GitHub URL formats
    // https://github.com/owner/repo/tree/main/path/to/skill
    // https://github.com/owner/repo
    
    let url = url.trim_end_matches('/');
    
    if url.starts_with("https://github.com/") {
        let parts: Vec<&str> = url.strip_prefix("https://github.com/")
            .unwrap_or("")
            .split('/')
            .collect();
        
        if parts.len() >= 2 {
            let owner = parts[0].to_string();
            let repo = parts[1].to_string();
            
            // Find path after /tree/main/ or /blob/main/
            let path = if parts.len() > 4 && (parts[2] == "tree" || parts[2] == "blob") {
                parts[4..].join("/")
            } else {
                String::new()
            };
            
            return Ok((owner, repo, path));
        }
    }
    
    bail!("Invalid GitHub URL format: {}", url)
}

/// Extract skill name from SKILL.md frontmatter
fn extract_skill_name(content: &str) -> Result<String> {
    if let Some(frontmatter) = content.strip_prefix("---").and_then(|s| s.split("---").next()) {
        for line in frontmatter.lines() {
            if let Some(name) = line.strip_prefix("name:") {
                return Ok(name.trim().to_string());
            }
        }
    }
    
    bail!("Could not extract skill name from frontmatter")
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}