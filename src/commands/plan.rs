//! Plan command handlers.

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

use crate::orchestrator::ToolOrchestrator;
use crate::planner::{PlanExecutor, TaskPlan};
use crate::runs::RunDatabase;

pub async fn handle_plan(goal: String, output: PathBuf) -> Result<()> {
    let plan = TaskPlan::from_goal(&goal);
    plan.write_json(&output)?;
    println!("Created plan: {}", output.display());
    println!("Goal: {}", plan.goal);
    println!("Steps: {}", plan.steps.len());
    Ok(())
}

pub async fn handle_run_plan(path: PathBuf, db_path: Option<PathBuf>) -> Result<()> {
    let plan = TaskPlan::read(&path)?;
    let db = RunDatabase::open(db_path.unwrap_or_else(RunDatabase::default_path))?;
    let orchestrator = Arc::new(ToolOrchestrator::new());
    let executor = PlanExecutor::new(orchestrator, db.clone());
    let report = executor.execute(&plan).await?;

    println!("Run ID: {}", report.run_id);
    println!("Status: {}", report.status);
    println!("Completed: {}", report.completed);
    println!("Failed: {}", report.failed);
    println!("Skipped: {}", report.skipped);
    println!("Run database: {}", db.path().display());
    Ok(())
}
