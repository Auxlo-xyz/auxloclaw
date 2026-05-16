//! Structured task plans and DAG execution.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use crate::orchestrator::{ToolOrchestrator, ToolResult};
use crate::runs::RunDatabase;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPlan {
    pub goal: String,
    #[serde(default)]
    pub strategy: Option<String>,
    pub steps: Vec<PlanStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub args: Value,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default = "default_step_retries")]
    pub retries: u32,
}

fn default_step_retries() -> u32 {
    0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanExecutionReport {
    pub run_id: String,
    pub status: String,
    pub completed: usize,
    pub failed: usize,
    pub skipped: usize,
}

impl TaskPlan {
    pub fn from_goal(goal: &str) -> Self {
        let slug = goal
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || c.is_whitespace() || *c == '-' || *c == '_')
            .collect::<String>();
        Self {
            goal: goal.to_string(),
            strategy: Some("default inspect-plan-execute skeleton; edit tool args before running destructive tasks".into()),
            steps: vec![
                PlanStep {
                    id: "inspect".into(),
                    description: format!("Inspect project context for: {}", slug.trim()),
                    tool: None,
                    args: Value::Null,
                    depends_on: vec![],
                    retries: 0,
                },
                PlanStep {
                    id: "execute".into(),
                    description: "Execute the planned implementation steps".into(),
                    tool: None,
                    args: Value::Null,
                    depends_on: vec!["inspect".into()],
                    retries: 0,
                },
                PlanStep {
                    id: "verify".into(),
                    description: "Run verification and summarize results".into(),
                    tool: None,
                    args: Value::Null,
                    depends_on: vec!["execute".into()],
                    retries: 0,
                },
            ],
        }
    }

    pub fn read(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read plan {}", path.display()))?;
        match path.extension().and_then(|e| e.to_str()) {
            Some("yaml") | Some("yml") => {
                serde_yaml::from_str(&content).context("failed to parse YAML plan")
            }
            _ => serde_json::from_str(&content).context("failed to parse JSON plan"),
        }
    }

    pub fn write_json(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)? + "\n")
            .with_context(|| format!("failed to write plan {}", path.display()))
    }

    pub fn validate(&self) -> Result<()> {
        if self.goal.trim().is_empty() {
            bail!("plan goal cannot be empty");
        }
        if self.steps.is_empty() {
            bail!("plan must include at least one step");
        }

        let mut ids = BTreeSet::new();
        for step in &self.steps {
            if step.id.trim().is_empty() {
                bail!("plan step id cannot be empty");
            }
            if !ids.insert(step.id.clone()) {
                bail!("duplicate plan step id: {}", step.id);
            }
            if step.description.trim().is_empty() {
                bail!("plan step {} description cannot be empty", step.id);
            }
        }

        for step in &self.steps {
            for dep in &step.depends_on {
                if !ids.contains(dep) {
                    bail!("step {} depends on unknown step {}", step.id, dep);
                }
                if dep == &step.id {
                    bail!("step {} cannot depend on itself", step.id);
                }
            }
        }

        self.execution_order()?;
        Ok(())
    }

    pub fn execution_order(&self) -> Result<Vec<String>> {
        let mut remaining: BTreeMap<String, Vec<String>> = self
            .steps
            .iter()
            .map(|s| (s.id.clone(), s.depends_on.clone()))
            .collect();
        let mut done = BTreeSet::new();
        let mut order = Vec::with_capacity(self.steps.len());

        while !remaining.is_empty() {
            let ready: Vec<String> = remaining
                .iter()
                .filter(|(_, deps)| deps.iter().all(|dep| done.contains(dep)))
                .map(|(id, _)| id.clone())
                .collect();

            if ready.is_empty() {
                bail!("plan contains a dependency cycle");
            }

            for id in ready {
                remaining.remove(&id);
                done.insert(id.clone());
                order.push(id);
            }
        }

        Ok(order)
    }
}

pub struct PlanExecutor {
    orchestrator: Arc<ToolOrchestrator>,
    runs: RunDatabase,
}

impl PlanExecutor {
    pub fn new(orchestrator: Arc<ToolOrchestrator>, runs: RunDatabase) -> Self {
        Self { orchestrator, runs }
    }

    pub async fn execute(&self, plan: &TaskPlan) -> Result<PlanExecutionReport> {
        plan.validate()?;
        let run_id = self.runs.start_run(
            "plan",
            &plan.goal,
            serde_json::json!({"strategy": plan.strategy, "step_count": plan.steps.len()}),
        )?;
        self.runs
            .log_event(&run_id, "plan_started", &plan.goal, serde_json::json!({}))?;

        for step in &plan.steps {
            self.runs
                .create_step(&run_id, &step.id, &step.description, step.tool.as_deref())?;
        }

        let by_id: BTreeMap<String, PlanStep> = plan
            .steps
            .iter()
            .map(|step| (step.id.clone(), step.clone()))
            .collect();
        let mut status: BTreeMap<String, String> = BTreeMap::new();
        let order = plan.execution_order()?;
        let mut completed = 0;
        let mut failed = 0;
        let mut skipped = 0;

        for step_id in order {
            let step = by_id
                .get(&step_id)
                .ok_or_else(|| anyhow!("missing step {}", step_id))?;
            if step
                .depends_on
                .iter()
                .any(|dep| status.get(dep).map(|s| s != "success").unwrap_or(true))
            {
                skipped += 1;
                status.insert(step.id.clone(), "skipped".into());
                self.runs.update_step(
                    &run_id,
                    &step.id,
                    "skipped",
                    serde_json::json!({"reason": "dependency failed"}),
                    None,
                )?;
                continue;
            }

            self.runs
                .update_step(&run_id, &step.id, "running", serde_json::json!({}), None)?;
            self.runs.log_event(
                &run_id,
                "step_started",
                &step.description,
                serde_json::json!({"step_id": step.id, "tool": step.tool}),
            )?;

            let result = self.execute_step(step).await;
            match result {
                Ok(tool_result) => {
                    if tool_result.success {
                        completed += 1;
                        status.insert(step.id.clone(), "success".into());
                        self.runs.update_step(
                            &run_id,
                            &step.id,
                            "success",
                            serde_json::to_value(&tool_result)?,
                            None,
                        )?;
                    } else {
                        failed += 1;
                        status.insert(step.id.clone(), "failed".into());
                        let err = tool_result
                            .error
                            .clone()
                            .unwrap_or_else(|| "tool returned failure".into());
                        self.runs.update_step(
                            &run_id,
                            &step.id,
                            "failed",
                            serde_json::to_value(&tool_result)?,
                            Some(&err),
                        )?;
                    }
                }
                Err(err) => {
                    failed += 1;
                    status.insert(step.id.clone(), "failed".into());
                    self.runs.update_step(
                        &run_id,
                        &step.id,
                        "failed",
                        serde_json::json!({}),
                        Some(&err.to_string()),
                    )?;
                }
            }
        }

        let final_status = if failed == 0 && skipped == 0 {
            "success"
        } else {
            "failed"
        };
        self.runs.finish_run(
            &run_id,
            final_status,
            Some(serde_json::json!({"completed": completed, "failed": failed, "skipped": skipped})),
        )?;

        Ok(PlanExecutionReport {
            run_id,
            status: final_status.into(),
            completed,
            failed,
            skipped,
        })
    }

    async fn execute_step(&self, step: &PlanStep) -> Result<ToolResult> {
        if let Some(tool) = &step.tool {
            let mut attempts = 0;
            loop {
                let result = self
                    .orchestrator
                    .execute_tool(tool, step.args.clone())
                    .await;
                if result.success || attempts >= step.retries {
                    return Ok(result);
                }
                attempts += 1;
            }
        }

        Ok(ToolResult {
            tool_name: "manual".into(),
            success: true,
            output: serde_json::json!({"description": step.description, "note": "manual/no-tool step recorded as complete"}),
            error: None,
            duration_ms: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_dependency_order() {
        let plan = TaskPlan {
            goal: "ship".into(),
            strategy: None,
            steps: vec![
                PlanStep {
                    id: "b".into(),
                    description: "B".into(),
                    tool: None,
                    args: Value::Null,
                    depends_on: vec!["a".into()],
                    retries: 0,
                },
                PlanStep {
                    id: "a".into(),
                    description: "A".into(),
                    tool: None,
                    args: Value::Null,
                    depends_on: vec![],
                    retries: 0,
                },
            ],
        };
        assert_eq!(plan.execution_order().unwrap(), vec!["a", "b"]);
    }

    #[test]
    fn rejects_cycles() {
        let plan = TaskPlan {
            goal: "cycle".into(),
            strategy: None,
            steps: vec![
                PlanStep {
                    id: "a".into(),
                    description: "A".into(),
                    tool: None,
                    args: Value::Null,
                    depends_on: vec!["b".into()],
                    retries: 0,
                },
                PlanStep {
                    id: "b".into(),
                    description: "B".into(),
                    tool: None,
                    args: Value::Null,
                    depends_on: vec!["a".into()],
                    retries: 0,
                },
            ],
        };
        assert!(plan.validate().is_err());
    }
}
