//! Regression tests for the per-session static system prompt freeze.
//!
//! 2026-08-12: `~/AGENTS.md` was appended twice mid-session by an external
//! writer. Because the static system prompt was rebuilt from disk on every API
//! call, each append changed the system prompt and flushed the entire provider
//! prefix cache (~160k tokens resent per call, flagged as
//! `harness:_system_changed` in KV_CACHE_USAGE telemetry). The static part is
//! now frozen per session; these tests pin that behavior.

use super::*;
use crate::message::{Message, ToolDefinition};
use crate::provider::{EventStream, Provider};
use crate::tool::Registry;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

struct PromptLockProvider;

#[async_trait]
impl Provider for PromptLockProvider {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        unreachable!("PromptLockProvider never completes requests")
    }

    fn name(&self) -> &str {
        "test"
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(PromptLockProvider)
    }
}

fn unique_temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "jcode-prompt-lock-{}-{}",
        std::process::id(),
        tag
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp prompt dir");
    dir
}

#[tokio::test]
async fn static_prompt_is_frozen_against_mid_session_agents_md_edits() {
    let _guard = crate::storage::lock_test_env();
    let dir = unique_temp_dir("freeze");
    std::fs::write(dir.join("AGENTS.md"), "# instructies versie A").unwrap();

    let provider: Arc<dyn Provider> = Arc::new(PromptLockProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);
    agent.session.working_dir = Some(dir.to_string_lossy().to_string());

    let first = agent.build_system_prompt_split(None);
    assert!(
        first.static_part.contains("versie A"),
        "eerste build moet de AGENTS.md van de working dir bevatten"
    );

    // Mid-session append zoals de Overleg-map writer van 2026-08-12.
    std::fs::write(
        dir.join("AGENTS.md"),
        "# instructies versie A\n\n## versie B die de cache niet mag flushen",
    )
    .unwrap();

    let second = agent.build_system_prompt_split(None);
    assert_eq!(
        first.static_part, second.static_part,
        "static prompt moet per sessie bevroren zijn, ongeacht disk-edits"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn static_prompt_lock_rebuilds_when_working_dir_changes() {
    let _guard = crate::storage::lock_test_env();
    let dir_a = unique_temp_dir("dir-a");
    let dir_b = unique_temp_dir("dir-b");
    std::fs::write(dir_a.join("AGENTS.md"), "# instructies van A").unwrap();
    std::fs::write(dir_b.join("AGENTS.md"), "# totaal andere B-instructies").unwrap();

    let provider: Arc<dyn Provider> = Arc::new(PromptLockProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);
    agent.session.working_dir = Some(dir_a.to_string_lossy().to_string());

    let first = agent.build_system_prompt_split(None);
    assert!(first.static_part.contains("instructies van A"));

    agent.session.working_dir = Some(dir_b.to_string_lossy().to_string());
    let second = agent.build_system_prompt_split(None);
    assert!(
        second.static_part.contains("totaal andere B-instructies"),
        "een nieuwe working dir moet de freeze bewust verversen"
    );

    let _ = std::fs::remove_dir_all(&dir_a);
    let _ = std::fs::remove_dir_all(&dir_b);
}

#[tokio::test]
async fn dynamic_parts_stay_per_call_on_a_frozen_static_prompt() {
    let _guard = crate::storage::lock_test_env();
    let dir = unique_temp_dir("dynamic");
    std::fs::write(dir.join("AGENTS.md"), "# instructies").unwrap();

    let provider: Arc<dyn Provider> = Arc::new(PromptLockProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);
    agent.session.working_dir = Some(dir.to_string_lossy().to_string());

    let plain = agent.build_system_prompt_split(None);
    let with_memory = agent.build_system_prompt_split(Some("# Memory\n\n- testnotitie"));

    assert_eq!(plain.static_part, with_memory.static_part);
    assert!(!plain.dynamic_part.contains("testnotitie"));
    assert!(with_memory.dynamic_part.contains("testnotitie"));

    let _ = std::fs::remove_dir_all(&dir);
}
