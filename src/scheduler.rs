//! Cron scheduler for autonomous recurring agent tasks.

use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::time::{timeout, Duration};
use tokio_cron_scheduler::{Job, JobScheduler};

use crate::agent::AgentCore;
use crate::config::{ScheduleJobConfig, SchedulerConfig};

pub struct CronScheduler {
    scheduler: JobScheduler,
}

impl CronScheduler {
    pub async fn start(agent: Arc<AgentCore>, config: SchedulerConfig) -> Result<Option<Self>> {
        if !config.enabled || config.jobs.is_empty() {
            return Ok(None);
        }

        let scheduler = JobScheduler::new()
            .await
            .context("failed to create cron scheduler")?;

        let mut registered = 0usize;
        for job_config in config.jobs.into_iter().filter(|job| job.enabled) {
            validate_job(&job_config)?;
            let cron = job_config.cron.clone();
            let name = job_config.name.clone();
            let prompt = job_config.prompt.clone();
            let session_id = job_config.session_id.clone();
            let timeout_secs = job_config.timeout_secs;
            let run_on_startup = job_config.run_on_startup;
            let job_agent = agent.clone();

            let job = Job::new_async(cron.as_str(), move |_uuid, _lock| {
                let agent = job_agent.clone();
                let name = name.clone();
                let prompt = prompt.clone();
                let session_id = session_id.clone();
                Box::pin(async move {
                    run_scheduled_job(agent, name, prompt, session_id, timeout_secs).await;
                })
            })
            .with_context(|| format!("invalid cron expression for job {}", job_config.name))?;

            scheduler
                .add(job)
                .await
                .with_context(|| format!("failed to add cron job {}", job_config.name))?;

            if run_on_startup {
                let startup_agent = agent.clone();
                let startup_name = job_config.name.clone();
                let startup_prompt = job_config.prompt.clone();
                let startup_session_id = job_config.session_id.clone();
                tokio::spawn(async move {
                    run_scheduled_job(
                        startup_agent,
                        startup_name,
                        startup_prompt,
                        startup_session_id,
                        timeout_secs,
                    )
                    .await;
                });
            }

            registered += 1;
        }

        if registered == 0 {
            return Ok(None);
        }

        scheduler
            .start()
            .await
            .context("failed to start cron scheduler")?;
        tracing::info!("⏰ Started cron scheduler with {} jobs", registered);
        Ok(Some(Self { scheduler }))
    }

    pub async fn shutdown(mut self) -> Result<()> {
        self.scheduler
            .shutdown()
            .await
            .context("failed to shut down cron scheduler")
    }
}

async fn run_scheduled_job(
    agent: Arc<AgentCore>,
    name: String,
    prompt: String,
    session_id: Option<String>,
    timeout_secs: u64,
) {
    tracing::info!("Running scheduled job: {}", name);
    let task = agent.process(&prompt, session_id.as_deref());
    match timeout(Duration::from_secs(timeout_secs), task).await {
        Ok(response) => tracing::info!(
            "Scheduled job complete: {} ({})",
            name,
            summarize(&response)
        ),
        Err(_) => tracing::error!("Scheduled job timed out: {} after {}s", name, timeout_secs),
    }
}

fn validate_job(job: &ScheduleJobConfig) -> Result<()> {
    if job.name.trim().is_empty() {
        anyhow::bail!("scheduled job name cannot be empty");
    }
    if job.cron.trim().is_empty() {
        anyhow::bail!("scheduled job {} cron cannot be empty", job.name);
    }
    if job.prompt.trim().is_empty() {
        anyhow::bail!("scheduled job {} prompt cannot be empty", job.name);
    }
    if job.timeout_secs == 0 {
        anyhow::bail!(
            "scheduled job {} timeout_secs must be greater than 0",
            job.name
        );
    }
    Ok(())
}

fn summarize(response: &str) -> String {
    const MAX: usize = 160;
    let compact = response.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.len() <= MAX {
        compact
    } else {
        format!("{}...", &compact[..MAX])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_job_config() {
        let job = ScheduleJobConfig {
            name: "heartbeat".into(),
            cron: "0 0/5 * * * *".into(),
            prompt: "Say heartbeat".into(),
            ..Default::default()
        };
        assert!(validate_job(&job).is_ok());
    }

    #[test]
    fn rejects_empty_prompt() {
        let job = ScheduleJobConfig {
            name: "bad".into(),
            cron: "0 0/5 * * * *".into(),
            prompt: String::new(),
            ..Default::default()
        };
        assert!(validate_job(&job).is_err());
    }

    #[test]
    fn summarizes_long_output() {
        let out = summarize(&"x ".repeat(200));
        assert!(out.ends_with("..."));
        assert!(out.len() <= 163);
    }
}
