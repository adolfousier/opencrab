use super::types::*;
use crate::brain::provider::Provider;
use crate::brain::tools::ToolRegistry;
use crate::services::ServiceContext;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Maximum number of recently-touched paths surfaced to the agent in
/// the system prompt for any one project. ~12 entries × ~30 tokens
/// each ≈ 400 tokens worst-case — small enough that the win on
/// cross-session continuity dominates the cost.
pub(super) const RECENT_PATHS_CAP: usize = 12;

/// Agent Service for managing AI conversations
pub struct AgentService {
    /// Default LLM provider — used for brand-new sessions that haven't
    /// had an explicit provider choice yet, and for channels / callers
    /// that invoke the agent without a session_id.
    pub(super) provider: std::sync::RwLock<Arc<dyn Provider>>,

    /// Per-session provider isolation. Every session that has ever been
    /// seen (via `/models` pick, `load_session`, or first agent turn)
    /// gets its own `Arc<dyn Provider>` here. In-flight agent turns
    /// read their session's entry via `provider_for_session(id)` so a
    /// foreground pane-switch or model-pick on a DIFFERENT session
    /// can't yank the active provider out from under a background
    /// turn. Before this map, `self.provider` was a single shared
    /// pointer — opening `/sessions` during a 47s cargo-clippy on one
    /// pane silently rewrote the running turn's endpoint to whatever
    /// the other session had saved (2026-04-17 17:01 logs).
    pub(super) session_providers: std::sync::RwLock<HashMap<Uuid, Arc<dyn Provider>>>,

    /// Per-session context window overrides. When a session's provider
    /// has a custom `configured_context_window()`, it's cached here so
    /// compaction and budget checks use the correct window even when
    /// the global provider changes (e.g. user switches models on another
    /// pane). Mirrors the `session_providers` pattern.
    pub(super) session_context_limits: std::sync::RwLock<HashMap<Uuid, u32>>,

    /// Service context for database operations
    pub(super) context: ServiceContext,

    /// Tool registry for executing tools
    pub(super) tool_registry: Arc<ToolRegistry>,

    /// Maximum tool execution iterations (0 = unlimited, relies on loop detection)
    pub(crate) max_tool_iterations: usize,

    /// System brain template
    pub(super) default_system_brain: Option<String>,

    /// Whether to auto-approve tool execution
    pub(super) auto_approve_tools: bool,

    /// Context window limit in tokens from config
    pub(super) context_limit: u32,

    /// Max output tokens for API calls from config
    pub(super) max_tokens: u32,

    /// Callback for requesting tool approval from user
    pub(super) approval_callback: Option<ApprovalCallback>,

    /// Callback for reporting progress during tool execution
    pub(super) progress_callback: Option<ProgressCallback>,

    /// Callback for checking queued user messages between tool iterations
    pub(super) message_queue_callback: Option<MessageQueueCallback>,

    /// Callback for requesting sudo password from user
    pub(super) sudo_callback: Option<SudoCallback>,

    /// Callback for requesting SSH password from user (for `ssh`, `scp`,
    /// `rsync` invocations whose key auth fails). Same shape as
    /// `sudo_callback` — wired by the TUI to a password dialog and by
    /// channels (future) to an approval card.
    pub(super) ssh_callback: Option<SshPasswordCallback>,

    /// Working directory for tool execution (shared, mutable at runtime via /cd or agent NLP)
    pub(super) working_directory: Arc<std::sync::RwLock<std::path::PathBuf>>,

    /// Brain path (~/.opencrabs/) for loading brain files
    pub(super) brain_path: Option<std::path::PathBuf>,

    /// Notification channel — fired after every `run_tool_loop` completion so
    /// the TUI can refresh when a remote channel (Telegram/WhatsApp/…) updates
    /// the shared session.
    pub(super) session_updated_tx:
        Option<tokio::sync::mpsc::UnboundedSender<super::types::ChannelSessionEvent>>,

    /// Fallback providers for rate-limit recovery (built from config on startup).
    /// When the primary provider hits a rate/account limit mid-stream, these are
    /// tried in order.
    pub(super) fallback_providers: Vec<Arc<dyn Provider>>,
}

impl AgentService {
    /// Create a new agent service. Reads agent settings from the provided config.
    pub async fn new(
        provider: Arc<dyn Provider>,
        context: ServiceContext,
        config: &crate::config::Config,
    ) -> Self {
        Self {
            provider: std::sync::RwLock::new(provider),
            session_providers: std::sync::RwLock::new(HashMap::new()),
            session_context_limits: std::sync::RwLock::new(HashMap::new()),
            context,
            tool_registry: Arc::new(ToolRegistry::new()),
            max_tool_iterations: 0, // 0 = unlimited (loop detection is the safety net)
            default_system_brain: None,
            auto_approve_tools: false,
            context_limit: config.agent.context_limit,
            max_tokens: config.agent.max_tokens,
            approval_callback: None,
            progress_callback: None,
            message_queue_callback: None,
            sudo_callback: None,
            ssh_callback: None,
            working_directory: Arc::new(std::sync::RwLock::new(
                std::env::current_dir().unwrap_or_default(),
            )),
            brain_path: None,
            session_updated_tx: None,
            fallback_providers: Self::build_fallback_providers(config).await,
        }
    }

    /// Create an agent service for tests (uses Config::default()).
    /// Only use in test code where no real user config exists.
    pub async fn new_for_test(provider: Arc<dyn Provider>, context: ServiceContext) -> Self {
        Self::new(provider, context, &crate::config::Config::default()).await
    }

    /// Get the service context
    pub fn context(&self) -> &ServiceContext {
        &self.context
    }

    /// Effective context-window budget. Returns the active provider's
    /// `configured_context_window()` when set (only custom OpenAI-compatible
    /// providers expose one, via `providers.<name>.context_window` in
    /// `config.toml`); otherwise the static `agent.context_limit`.
    ///
    /// Prefer `context_limit_for_session(session_id)` for session-scoped
    /// operations (compaction, budget checks) to avoid cross-session
    /// contamination when the global provider changes.
    pub fn context_limit(&self) -> u32 {
        self.provider()
            .configured_context_window()
            .unwrap_or(self.context_limit)
    }

    /// Per-session context window budget. Mirrors `provider_for_session`:
    /// returns the cached override for this session if one exists (set by
    /// `swap_provider_for_session`), otherwise falls back to the global
    /// `context_limit()`. This ensures compaction and budget checks use
    /// the correct window even when the user switches models on another pane.
    pub fn context_limit_for_session(&self, session_id: Uuid) -> u32 {
        if let Ok(map) = self.session_context_limits.read()
            && let Some(&cw) = map.get(&session_id)
        {
            return cw;
        }
        self.context_limit()
    }

    /// Get max tokens from config
    pub fn max_tokens(&self) -> u32 {
        self.max_tokens
    }

    /// Get the tool registry
    pub fn tool_registry(&self) -> &Arc<ToolRegistry> {
        &self.tool_registry
    }

    /// Get the progress callback (for preserving across rebuilds)
    pub fn progress_callback(&self) -> &Option<ProgressCallback> {
        &self.progress_callback
    }

    /// Get the message queue callback (for preserving across rebuilds)
    pub fn message_queue_callback(&self) -> &Option<MessageQueueCallback> {
        &self.message_queue_callback
    }

    /// Get the sudo callback (for preserving across rebuilds)
    pub fn sudo_callback(&self) -> &Option<SudoCallback> {
        &self.sudo_callback
    }

    /// Get the SSH password callback (for preserving across rebuilds)
    pub fn ssh_callback(&self) -> &Option<SshPasswordCallback> {
        &self.ssh_callback
    }

    /// Get the working directory (for preserving across rebuilds)
    pub fn working_directory(&self) -> &Arc<std::sync::RwLock<std::path::PathBuf>> {
        &self.working_directory
    }

    /// Get the brain path (for preserving across rebuilds)
    pub fn brain_path(&self) -> &Option<std::path::PathBuf> {
        &self.brain_path
    }

    /// Set the default system brain
    pub fn with_system_brain(mut self, prompt: String) -> Self {
        self.default_system_brain = Some(prompt);
        self
    }

    /// Set maximum tool iterations
    pub fn with_max_tool_iterations(mut self, max: usize) -> Self {
        self.max_tool_iterations = max;
        self
    }

    /// Set the tool registry
    pub fn with_tool_registry(mut self, registry: Arc<ToolRegistry>) -> Self {
        self.tool_registry = registry;
        self
    }

    /// Set whether to auto-approve tool execution
    pub fn with_auto_approve_tools(mut self, auto_approve: bool) -> Self {
        self.auto_approve_tools = auto_approve;
        self
    }

    /// Set the approval callback for interactive tool approval
    pub fn with_approval_callback(mut self, callback: Option<ApprovalCallback>) -> Self {
        self.approval_callback = callback;
        self
    }

    /// Set the progress callback for reporting tool execution progress
    pub fn with_progress_callback(mut self, callback: Option<ProgressCallback>) -> Self {
        self.progress_callback = callback;
        self
    }

    /// Set the message queue callback for injecting user messages between tool iterations
    pub fn with_message_queue_callback(mut self, callback: Option<MessageQueueCallback>) -> Self {
        self.message_queue_callback = callback;
        self
    }

    /// Set the sudo password callback for interactive sudo prompts
    pub fn with_sudo_callback(mut self, callback: Option<SudoCallback>) -> Self {
        self.sudo_callback = callback;
        self
    }

    /// Set the SSH password callback for interactive ssh/scp/rsync prompts
    pub fn with_ssh_callback(mut self, callback: Option<SshPasswordCallback>) -> Self {
        self.ssh_callback = callback;
        self
    }

    /// Set the working directory for tool execution
    pub fn with_working_directory(self, working_directory: std::path::PathBuf) -> Self {
        *self
            .working_directory
            .write()
            .expect("working_directory lock poisoned") = working_directory;
        self
    }

    /// Get the current working directory
    pub fn get_working_directory(&self) -> std::path::PathBuf {
        self.working_directory
            .read()
            .expect("working_directory lock poisoned")
            .clone()
    }

    /// Change the working directory at runtime (called from /cd or agent tools)
    pub fn set_working_directory(&self, path: std::path::PathBuf) {
        *self
            .working_directory
            .write()
            .expect("working_directory lock poisoned") = path;
    }

    /// Get a shared handle to the working directory (for tools that need to mutate it)
    pub fn shared_working_directory(&self) -> Arc<std::sync::RwLock<std::path::PathBuf>> {
        Arc::clone(&self.working_directory)
    }

    /// Set the brain path (~/.opencrabs/)
    pub fn with_brain_path(mut self, brain_path: std::path::PathBuf) -> Self {
        self.brain_path = Some(brain_path);
        self
    }

    /// Set the session-updated notification sender.
    ///
    /// When set, `run_tool_loop` fires this after every completed agent response
    /// so the TUI can reload the session in real-time when a remote channel
    /// (Telegram, WhatsApp, Discord, Slack) processes a message.
    pub fn with_session_updated_tx(
        mut self,
        tx: tokio::sync::mpsc::UnboundedSender<super::types::ChannelSessionEvent>,
    ) -> Self {
        self.session_updated_tx = Some(tx);
        self
    }

    /// Get the session-updated sender (for preserving across agent rebuilds).
    pub fn session_updated_tx(
        &self,
    ) -> Option<tokio::sync::mpsc::UnboundedSender<super::types::ChannelSessionEvent>> {
        self.session_updated_tx.clone()
    }

    /// Get the provider name. When a sticky FallbackProvider has swapped to
    /// a fallback, this returns the *active* sub-provider's name so the
    /// footer/splash reflects what's actually serving requests.
    pub fn provider_name(&self) -> String {
        let p = self.provider.read().expect("provider lock poisoned");
        p.active_subprovider_name()
            .unwrap_or_else(|| p.name().to_string())
    }

    /// Get the system brain
    pub fn system_brain(&self) -> Option<&String> {
        self.default_system_brain.as_ref()
    }

    /// Estimate the baseline token cost of every request for this agent:
    /// system prompt + tool definitions. This is the floor for the ctx display
    /// even on a brand-new session with no messages.
    pub fn base_context_tokens(&self) -> u32 {
        use crate::brain::tokenizer::count_tokens;
        let system_tokens = self
            .default_system_brain
            .as_deref()
            .map(count_tokens)
            .unwrap_or(0);
        let tool_tokens = self.actual_tool_schema_tokens();
        (system_tokens + tool_tokens) as u32
    }

    /// Get the default model for this provider. Mirrors `provider_name()`
    /// — returns the sticky-fallback active model when swapped.
    pub fn provider_model(&self) -> String {
        let p = self.provider.read().expect("provider lock poisoned");
        p.active_subprovider_model()
            .unwrap_or_else(|| p.default_model().to_string())
    }

    /// Get the list of supported models for this provider (hardcoded fallback)
    pub fn supported_models(&self) -> Vec<String> {
        self.provider
            .read()
            .expect("provider lock poisoned")
            .supported_models()
    }

    /// Fetch available models from the provider API (live)
    pub async fn fetch_models(&self) -> Vec<String> {
        let provider = self
            .provider
            .read()
            .expect("provider lock poisoned")
            .clone();
        provider.fetch_models().await
    }

    /// Get a clone of the underlying LLM provider
    pub fn provider(&self) -> Arc<dyn Provider> {
        self.provider
            .read()
            .expect("provider lock poisoned")
            .clone()
    }

    /// Swap the DEFAULT provider at runtime. Used during bootstrap and by
    /// callers without a session_id. Prefer `swap_provider_for_session` for
    /// anything session-scoped — this does NOT affect sessions that already
    /// have their own entry in `session_providers`.
    pub fn swap_provider(&self, new_provider: Arc<dyn Provider>) {
        *self.provider.write().expect("provider lock poisoned") = new_provider;
    }

    /// Look up the provider a specific session should use. Returns the
    /// session's dedicated entry if one exists; otherwise falls back to
    /// the global default. Read-path hot function — cheap Arc clone,
    /// no allocation beyond lock acquisition.
    pub fn provider_for_session(&self, session_id: Uuid) -> Arc<dyn Provider> {
        if let Ok(map) = self.session_providers.read()
            && let Some(p) = map.get(&session_id)
        {
            return p.clone();
        }
        self.provider
            .read()
            .expect("provider lock poisoned")
            .clone()
    }

    /// Assign a provider specifically to `session_id`. Subsequent agent
    /// turns for that session use this provider; other sessions and the
    /// global default are untouched. Called by `/models` dialog on model
    /// pick and by `load_session` when restoring a session's saved
    /// `provider_name`.
    ///
    /// Also caches the provider's `configured_context_window()` into
    /// `session_context_limits` so compaction uses the correct budget
    /// even if the global provider changes later.
    pub fn swap_provider_for_session(&self, session_id: Uuid, new_provider: Arc<dyn Provider>) {
        self.session_providers
            .write()
            .expect("session_providers lock poisoned")
            .insert(session_id, new_provider.clone());

        // Cache context window for this session
        if let Some(cw) = new_provider.configured_context_window() {
            self.session_context_limits
                .write()
                .expect("session_context_limits lock poisoned")
                .insert(session_id, cw);
        }
    }

    /// Drop a session's provider entry (e.g. session deleted). Noop if
    /// no entry exists. Does NOT affect the global default or other
    /// sessions.
    pub fn remove_session_provider(&self, session_id: Uuid) {
        self.session_providers
            .write()
            .expect("session_providers lock poisoned")
            .remove(&session_id);
        self.session_context_limits
            .write()
            .expect("session_context_limits lock poisoned")
            .remove(&session_id);
    }

    /// Snapshot of every per-session provider binding. Used by
    /// `rebuild_agent_service` to carry session→provider pins across
    /// the rebuild so live sessions on other panes don't lose their
    /// provider when the user reconfigures via `/models`.
    pub fn session_provider_snapshot(&self) -> Vec<(Uuid, Arc<dyn Provider>)> {
        let map = self
            .session_providers
            .read()
            .expect("session_providers lock poisoned");
        map.iter().map(|(k, v)| (*k, v.clone())).collect()
    }

    /// Provider name for a specific session, including sticky-fallback
    /// active sub-provider.
    pub fn provider_name_for_session(&self, session_id: Uuid) -> String {
        let p = self.provider_for_session(session_id);
        p.active_subprovider_name()
            .unwrap_or_else(|| p.name().to_string())
    }

    /// Default model for a specific session, including sticky-fallback
    /// active sub-model.
    pub fn provider_model_for_session(&self, session_id: Uuid) -> String {
        let p = self.provider_for_session(session_id);
        p.active_subprovider_model()
            .unwrap_or_else(|| p.default_model().to_string())
    }

    /// Persist a sticky-fallback {provider, model} pair onto the session row
    /// in the DB. Called at every site that emits `ProviderSwitched` so the
    /// pair is durable regardless of whether the consuming progress callback
    /// (TUI / Slack / Telegram / Discord / WhatsApp) actually handles the
    /// event — channel handlers historically only matched on ToolStarted /
    /// IntermediateText / SelfHealingAlert and dropped ProviderSwitched on
    /// the floor, leaving DB on the pre-fallback pair while
    /// `session_providers[sid]` was already swapped to the fallback. The
    /// next channel turn would then see DB-and-memory disagree, run
    /// `sync_provider_for_session` to "restore" memory from stale DB, and
    /// loop forever — eventually leaking a model from one provider's
    /// catalogue onto another provider's session row when an update path
    /// finally caught up out of order.
    ///
    /// Fire-and-forget: spawns a tokio task. The pair is written once; if
    /// it fails, the next ProviderSwitched (or response writeback) will
    /// retry. We don't block the streaming path on the DB roundtrip.
    pub(crate) fn persist_sticky_pair(
        &self,
        session_id: Uuid,
        provider_name: String,
        model: String,
    ) {
        let context = self.context.clone();
        tokio::spawn(async move {
            let svc = crate::services::SessionService::new(context);
            match svc.get_session(session_id).await {
                Ok(Some(mut s)) => {
                    s.provider_name = Some(provider_name.clone());
                    s.model = Some(model.clone());
                    if let Err(e) = svc.update_session(&s).await {
                        tracing::warn!(
                            "persist_sticky_pair[{}]: update_session failed: {}",
                            session_id,
                            e
                        );
                    }
                }
                Ok(None) => {
                    tracing::warn!(
                        "persist_sticky_pair[{}]: session not found in DB",
                        session_id
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "persist_sticky_pair[{}]: get_session failed: {}",
                        session_id,
                        e
                    );
                }
            }
        });
    }

    /// Get context window size for a given model.
    ///
    /// Delegates to `context_limit()` so custom OpenAI-compatible providers
    /// that declare a `providers.custom.<name>.context_window` are honored
    /// here too. Without this, the TUI header reads the static
    /// `agent.context_limit` fallback (typically 200k) while the actual
    /// budget enforcer uses the provider-configured window — producing a
    /// misleading "202k/200k" when the engine is still safely inside its
    /// real limit.
    pub fn context_window_for_model(&self, _model: &str) -> u32 {
        self.context_limit()
    }

    /// Record that the agent just successfully accessed `raw_path`
    /// while operating under `working_directory`. Persists to the
    /// `recent_paths` table so a later session on the same project
    /// can re-anchor on real paths instead of guessing.
    ///
    /// Fire-and-forget: spawns a task and never blocks the tool loop.
    /// Both the working directory and the path are collapsed to
    /// `~/...` form before storage so the key is stable across
    /// machines and OS user names.
    pub fn record_recent_path(
        &self,
        working_directory: &std::path::Path,
        raw_path: &std::path::Path,
    ) {
        let wd_collapsed = crate::brain::tools::error::collapse_home(working_directory);
        let path_collapsed = crate::brain::tools::error::collapse_home(raw_path);
        if wd_collapsed.is_empty() || path_collapsed.is_empty() {
            return;
        }
        let pool = self.context.pool();
        tokio::spawn(async move {
            let repo = crate::db::repository::RecentPathsRepository::new(pool);
            if let Err(e) = repo.record(&wd_collapsed, &path_collapsed).await {
                tracing::debug!("recent_paths write failed: {e}");
            }
        });
    }

    /// Top recently-accessed paths under the given `working_directory`,
    /// most-recent first, capped at `RECENT_PATHS_CAP`. Returns an empty
    /// Vec when the project has no recorded paths yet (or on DB error).
    /// Stored & returned in `~/...` collapsed form.
    pub async fn recent_paths_for_dir(&self, working_directory: &std::path::Path) -> Vec<String> {
        let wd_collapsed = crate::brain::tools::error::collapse_home(working_directory);
        if wd_collapsed.is_empty() {
            return Vec::new();
        }
        let repo = crate::db::repository::RecentPathsRepository::new(self.context.pool());
        match repo.top_for_dir(&wd_collapsed, RECENT_PATHS_CAP).await {
            Ok(paths) => paths,
            Err(e) => {
                tracing::debug!("recent_paths read failed: {e}");
                Vec::new()
            }
        }
    }

    /// Build fallback providers from config for mid-stream rate limit recovery.
    async fn build_fallback_providers(config: &crate::config::Config) -> Vec<Arc<dyn Provider>> {
        if let Some(fallback) = &config.providers.fallback
            && fallback.enabled
        {
            let chain = crate::brain::provider::factory::fallback_chain(fallback);
            let mut providers = Vec::new();
            for name in &chain {
                match crate::brain::provider::factory::create_provider_by_name(config, name).await {
                    Ok(p) => {
                        tracing::info!("AgentService: fallback provider '{}' ready", name);
                        providers.push(p);
                    }
                    Err(e) => {
                        tracing::warn!("AgentService: fallback provider '{}' skipped: {}", name, e);
                    }
                }
            }
            providers
        } else {
            Vec::new()
        }
    }

    /// Check if any fallback providers are configured
    pub fn has_fallback_provider(&self) -> bool {
        !self.fallback_providers.is_empty()
    }

    /// Get the next fallback provider that isn't the currently active one.
    /// Walks the chain until it finds a different provider name.
    pub fn try_get_fallback_provider(&self) -> Option<Arc<dyn Provider>> {
        let active_name = self
            .provider
            .read()
            .ok()
            .map(|p| p.name().to_string())
            .unwrap_or_default();
        self.fallback_providers
            .iter()
            .find(|p| p.name() != active_name)
            .cloned()
    }
}
