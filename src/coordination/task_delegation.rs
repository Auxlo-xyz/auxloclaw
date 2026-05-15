//! Task Delegation Logic
//! Analyzes tasks and determines optimal routing

use std::collections::BTreeMap;

/// Task priority levels
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskPriority {
    Low,
    Normal,
    High,
    Critical,
}

/// Delegated task with metadata
#[derive(Debug, Clone)]
pub struct DelegatedTask {
    pub task: String,
    pub agent_type: String,
    pub priority: TaskPriority,
    pub estimated_tools: u32,
    pub dependencies: Vec<String>,
    pub parallel_safe: bool,
}

/// Task delegator with classification rules
pub struct TaskDelegator {
    /// Keyword mappings for agent types
    agent_keywords: BTreeMap<String, Vec<String>>,
    /// Tasks that should always be delegated
    delegate_patterns: Vec<String>,
    /// Tasks that should stay on main agent
    main_agent_patterns: Vec<String>,
}

impl TaskDelegator {
    pub fn new() -> Self {
        let mut agent_keywords = BTreeMap::new();

        agent_keywords.insert(
            "researcher".to_string(),
            vec![
                "research".to_string(),
                "find information".to_string(),
                "search for".to_string(),
                "look up".to_string(),
                "analyze sources".to_string(),
                "gather data".to_string(),
                "what is".to_string(),
                "explain".to_string(),
                "compare".to_string(),
                "investigate".to_string(),
            ],
        );

        agent_keywords.insert(
            "coder".to_string(),
            vec![
                "write code".to_string(),
                "implement".to_string(),
                "fix bug".to_string(),
                "debug".to_string(),
                "refactor".to_string(),
                "optimize".to_string(),
                "create function".to_string(),
                "script".to_string(),
                "programming".to_string(),
                "develop".to_string(),
            ],
        );

        agent_keywords.insert(
            "analyst".to_string(),
            vec![
                "analyze".to_string(),
                "interpret".to_string(),
                "data".to_string(),
                "statistics".to_string(),
                "metrics".to_string(),
                "report".to_string(),
                "trends".to_string(),
                "patterns".to_string(),
                "visualization".to_string(),
                "insights".to_string(),
            ],
        );

        agent_keywords.insert(
            "planner".to_string(),
            vec![
                "plan".to_string(),
                "organize".to_string(),
                "schedule".to_string(),
                "roadmap".to_string(),
                "break down".to_string(),
                "strategy".to_string(),
                "steps".to_string(),
                "workflow".to_string(),
            ],
        );

        agent_keywords.insert(
            "reviewer".to_string(),
            vec![
                "review".to_string(),
                "check".to_string(),
                "audit".to_string(),
                "validate".to_string(),
                "test".to_string(),
                "verify".to_string(),
                "quality".to_string(),
                "improve".to_string(),
            ],
        );

        let delegate_patterns = vec![
            "in parallel".to_string(),
            "separately".to_string(),
            "independently".to_string(),
            "as a sub-task".to_string(),
            "delegate".to_string(),
            "spawn agent".to_string(),
        ];

        let main_agent_patterns = vec![
            "remember".to_string(),
            "recall".to_string(),
            "previous conversation".to_string(),
            "our discussion".to_string(),
            "you said".to_string(),
            "continue from".to_string(),
        ];

        Self {
            agent_keywords,
            delegate_patterns,
            main_agent_patterns,
        }
    }

    /// Classify task to determine best agent type
    pub fn classify_task(&self, task: &str) -> String {
        let task_lower = task.to_lowercase();

        // Prioritize agent types in a specific order to ensure deterministic and logical classification
        let priority_order = vec!["researcher", "coder", "analyst", "planner", "reviewer"];

        for agent_type in priority_order {
            if let Some(keywords) = self.agent_keywords.get(agent_type) {
                for keyword in keywords {
                    if task_lower.contains(&keyword.to_lowercase()) {
                        return agent_type.to_string();
                    }
                }
            }
        }

        // Default to general
        "general".to_string()
    }

    /// Check if task should be delegated
    pub fn should_delegate(&self, task: &str) -> bool {
        let task_lower = task.to_lowercase();

        // Check for explicit delegation patterns
        for pattern in &self.delegate_patterns {
            if task_lower.contains(&pattern.to_lowercase()) {
                return true;
            }
        }

        // Check if it's a complex multi-step task
        if self.is_complex_task(task) {
            return true;
        }

        // Check if task matches a specialized agent
        let agent_type = self.classify_task(task);
        agent_type != "general"
    }

    /// Check if task should stay on main agent
    pub fn should_keep_on_main(&self, task: &str) -> bool {
        let task_lower = task.to_lowercase();

        for pattern in &self.main_agent_patterns {
            if task_lower.contains(&pattern.to_lowercase()) {
                return true;
            }
        }

        false
    }

    /// Determine if task is complex enough to delegate
    fn is_complex_task(&self, task: &str) -> bool {
        // Heuristics for complexity
        let word_count = task.split_whitespace().count();
        let has_multiple_verbs = [" and ", " then ", " after ", " before ", " while "]
            .iter()
            .filter(|sep| task.to_lowercase().contains(*sep))
            .count()
            > 1;

        let has_subtasks = task.contains(",") || task.contains("\n");

        word_count > 50 || has_multiple_verbs || has_subtasks
    }

    /// Get priority for task
    pub fn get_priority(&self, task: &str) -> TaskPriority {
        let task_lower = task.to_lowercase();

        if task_lower.contains("urgent")
            || task_lower.contains("critical")
            || task_lower.contains("asap")
        {
            TaskPriority::Critical
        } else if task_lower.contains("important") || task_lower.contains("priority") {
            TaskPriority::High
        } else if task_lower.contains("when possible") || task_lower.contains("later") {
            TaskPriority::Low
        } else {
            TaskPriority::Normal
        }
    }

    /// Estimate number of tools needed
    pub fn estimate_tool_count(&self, task: &str) -> u32 {
        let task_lower = task.to_lowercase();
        let mut count = 1u32;

        // Each keyword match suggests a tool might be needed
        for (_, keywords) in &self.agent_keywords {
            for keyword in keywords {
                if task_lower.contains(&keyword.to_lowercase()) {
                    count += 1;
                }
            }
        }

        count.min(10)
    }

    /// Create full delegated task metadata
    pub fn create_delegated_task(&self, task: &str) -> DelegatedTask {
        let agent_type = self.classify_task(task);
        let priority = self.get_priority(task);
        let estimated_tools = self.estimate_tool_count(task);

        DelegatedTask {
            task: task.to_string(),
            agent_type: agent_type.clone(),
            priority,
            estimated_tools,
            dependencies: vec![],
            parallel_safe: agent_type == "researcher" || agent_type == "analyst",
        }
    }

    /// Split complex task into sub-tasks
    pub fn split_task(&self, task: &str) -> Vec<DelegatedTask> {
        let mut sub_tasks = Vec::new();

        // Split by common delimiters
        let delimiters = [" then ", " and then ", " after that ", " next, "];
        let mut parts = vec![task.to_string()];

        for delim in &delimiters {
            let mut new_parts = Vec::new();
            for part in &parts {
                if part.to_lowercase().contains(delim) {
                    for sub_part in part.split(delim) {
                        new_parts.push(sub_part.trim().to_string());
                    }
                } else {
                    new_parts.push(part.clone());
                }
            }
            parts = new_parts;
        }

        // Create delegated tasks for each part
        for part in parts {
            if !part.is_empty() {
                sub_tasks.push(self.create_delegated_task(&part));
            }
        }

        if sub_tasks.is_empty() {
            sub_tasks.push(self.create_delegated_task(task));
        }

        sub_tasks
    }
}

impl Default for TaskDelegator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_classification() {
        let delegator = TaskDelegator::new();

        assert_eq!(
            delegator.classify_task("Research the latest AI trends"),
            "researcher"
        );
        assert_eq!(
            delegator.classify_task("Write code for a REST API"),
            "coder"
        );
        assert_eq!(delegator.classify_task("Analyze the sales data"), "analyst");
    }

    #[test]
    fn test_should_delegate() {
        let delegator = TaskDelegator::new();

        assert!(delegator.should_delegate("Research and analyze the market"));
        assert!(!delegator.should_delegate("What did you say earlier?"));
    }
}
