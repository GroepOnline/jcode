#[cfg(test)]
#[path = "prompting_tests.rs"]
mod prompting_tests;

use super::Agent;
use crate::logging;
use crate::message::{Message, ToolDefinition};

/// Frozen static system prompt for a session. Rebuilt only when the inputs
/// that legitimately redefine it change (skills set, selfdev mode, working
/// directory) — never because a prompt source file changed on disk
/// mid-session. See `Agent::locked_static_prompt`.
pub(super) struct StaticPromptLock {
    pub(super) static_part: String,
    pub(super) skills_fingerprint: u64,
    pub(super) is_selfdev: bool,
    pub(super) working_dir: Option<String>,
    pub(super) config_generation: u64,
}

impl Agent {
    pub(super) fn log_prompt_prefix_accounting(
        &self,
        split: &crate::prompt::SplitSystemPrompt,
        tools: &[ToolDefinition],
    ) {
        let system_tokens = split.estimated_tokens();
        let tool_tokens = ToolDefinition::aggregate_prompt_token_estimate(tools);
        let prefix_tokens = system_tokens + tool_tokens;
        logging::info(&format!(
            "Prompt prefix estimate: total={} tokens (system={} tools={})",
            prefix_tokens, system_tokens, tool_tokens
        ));
    }

    pub(super) fn build_memory_prompt_nonblocking_shared(
        &self,
        messages: std::sync::Arc<[Message]>,
        _memory_event_tx: Option<crate::memory::MemoryEventSink>,
    ) -> Option<crate::memory::PendingMemory> {
        if !self.memory_enabled {
            return None;
        }

        let session_id = &self.session.id;

        let fresh_user_turn = crate::message::ends_with_fresh_user_turn(&messages);
        let pending = if fresh_user_turn {
            crate::memory::take_pending_memory(session_id)
        } else {
            None
        };

        // Use the persistent memory-agent pipeline as the single source of truth.
        // Running both this and the legacy MemoryManager background retrieval path
        // can prepare overlapping pending prompts for the same turn, which makes
        // memory injection feel overly aggressive.
        // Relevance results are consumed only at the start of a fresh user turn.
        // Enqueuing again after every tool result runs the local embedding model
        // for each provider continuation without creating an additional injection
        // opportunity. One update per user turn keeps memory current while avoiding
        // redundant 512-token inference during tool-heavy agent loops.
        if fresh_user_turn {
            crate::memory_agent::update_context_sync_with_dir(
                session_id,
                messages,
                self.session.working_dir.clone(),
            );
        }

        pending
    }

    fn append_current_turn_system_reminder(&self, split: &mut crate::prompt::SplitSystemPrompt) {
        let Some(reminder) = self
            .current_turn_system_reminder
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        else {
            return;
        };

        if !split.dynamic_part.is_empty() {
            split.dynamic_part.push_str("\n\n");
        }
        split.dynamic_part.push_str("# System Reminder\n\n");
        split.dynamic_part.push_str(reminder);
    }

    /// Build split system prompt for better caching
    /// Returns static (cacheable) and dynamic (not cached) parts separately
    pub(super) fn build_system_prompt_split(
        &self,
        memory_prompt: Option<&str>,
    ) -> crate::prompt::SplitSystemPrompt {
        if let Some(ref override_prompt) = self.system_prompt_override {
            return crate::prompt::SplitSystemPrompt {
                static_part: override_prompt.clone(),
                dynamic_part: String::new(),
            };
        }

        let skills = self.current_skills_snapshot();
        let skill_prompt = self
            .active_skill
            .as_ref()
            .and_then(|name| skills.get(name).map(|skill| skill.get_prompt().to_string()));

        let available_skills: Vec<crate::prompt::SkillInfo> = self
            .current_skills_snapshot()
            .list()
            .iter()
            .map(|skill| crate::prompt::SkillInfo {
                name: skill.name.clone(),
                description: skill.description.clone(),
            })
            .collect();

        let working_dir = self
            .session
            .working_dir
            .as_ref()
            .map(std::path::PathBuf::from);

        // The static part embeds files read from disk (AGENTS.md, .jcode
        // overlays, preferred-tools). Re-reading them per API call made the
        // system prompt silently change mid-session whenever something edited
        // those files — observed live 2026-08-12: two `~/AGENTS.md` appends
        // during one turn each flushed the full provider prompt cache
        // (~160k tokens resent per call on cline-pass). Freeze the static part
        // per session (same philosophy as `locked_tools`); key it on the
        // inputs that legitimately redefine it so those still rebuild.
        let fingerprint = crate::prompt::skills_list_fingerprint(&available_skills);
        let is_selfdev = self.session.is_canary;
        let working_dir_key = self.session.working_dir.clone();
        let config_generation = crate::config::config_reload_generation();

        let static_part = {
            let mut lock = self
                .locked_static_prompt
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match lock.as_ref() {
                Some(frozen)
                    if frozen.skills_fingerprint == fingerprint
                        && frozen.is_selfdev == is_selfdev
                        && frozen.working_dir == working_dir_key
                        && frozen.config_generation == config_generation =>
                {
                    frozen.static_part.clone()
                }
                _ => {
                    let (split, _context_info) = crate::prompt::build_system_prompt_split(
                        None,
                        &available_skills,
                        is_selfdev,
                        None,
                        working_dir.as_deref(),
                    );
                    let static_part = split.static_part;
                    *lock = Some(StaticPromptLock {
                        static_part: static_part.clone(),
                        skills_fingerprint: fingerprint,
                        is_selfdev,
                        working_dir: working_dir_key,
                        config_generation,
                    });
                    static_part
                }
            }
        };

        let mut split = crate::prompt::SplitSystemPrompt {
            static_part,
            dynamic_part: String::new(),
        };
        crate::prompt::append_dynamic_prompt_parts(
            &mut split,
            memory_prompt,
            skill_prompt.as_deref(),
        );

        self.append_current_turn_system_reminder(&mut split);
        crate::prompt::append_swarm_effort_directive(
            &mut split,
            self.provider.reasoning_effort().as_deref(),
        );

        split
    }

    /// Non-blocking memory prompt - takes pending result and spawns check for next turn
    #[cfg(test)]
    pub(super) fn build_memory_prompt_nonblocking(
        &self,
        messages: &[Message],
        _memory_event_tx: Option<crate::memory::MemoryEventSink>,
    ) -> Option<crate::memory::PendingMemory> {
        self.build_memory_prompt_nonblocking_shared(messages.to_vec().into(), _memory_event_tx)
    }
}
