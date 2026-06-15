//! CLI subcommands — run, init, config, db, keyring, logs, status, doctor, and config loading.

use anyhow::{Context, Result};
use std::sync::Arc;

use crate::brain::BrainLoader;
use crate::brain::prompt_builder::RuntimeInfo;

use super::args::{
    ChannelCommands, DbCommands, LogCommands, MemoryCommands, OutputFormat, ProfileCommands,
    ServiceCommands, SessionCommands,
};

/// Show system status
pub(crate) async fn cmd_status(config: &crate::config::Config) -> Result<()> {
    use crate::db::Database;

    let version = env!("CARGO_PKG_VERSION");
    println!("🦀 OpenCrabs v{version}\n");

    // Provider
    match crate::brain::provider::create_provider(config).await {
        Ok(provider) => {
            println!(
                "  Provider:  {} ({})",
                provider.name(),
                provider.default_model()
            );
        }
        Err(_) => println!("  Provider:  not configured"),
    }

    // Brain
    let brain_path = BrainLoader::resolve_path();
    let brain_files: Vec<&str> = [
        "persona.md",
        "system.md",
        "USER.md",
        "MEMORY.md",
        "AGENTS.md",
        "TOOLS.md",
        "SOUL.md",
    ]
    .iter()
    .filter(|f| brain_path.join(f).exists())
    .copied()
    .collect();
    if brain_files.is_empty() {
        println!("  Brain:     no files found at {}", brain_path.display());
    } else {
        println!(
            "  Brain:     {} files ({})",
            brain_files.len(),
            brain_files.join(", ")
        );
    }

    // Database
    let db_path = &config.database.path;
    if db_path.exists() {
        let size = std::fs::metadata(db_path).map(|m| m.len()).unwrap_or(0);
        let size_str = if size > 1_048_576 {
            format!("{:.1} MB", size as f64 / 1_048_576.0)
        } else {
            format!("{:.0} KB", size as f64 / 1024.0)
        };

        match Database::connect(db_path).await {
            Ok(db) => {
                let counts = async {
                    let conn = db.pool().get().await.ok()?;
                    conn.interact(|c| {
                        let sessions: i64 =
                            c.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))?;
                        let messages: i64 =
                            c.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))?;
                        Ok::<_, rusqlite::Error>((sessions, messages))
                    })
                    .await
                    .ok()?
                    .ok()
                }
                .await;
                if let Some((sessions, messages)) = counts {
                    println!(
                        "  Database:  {} — {} sessions, {} messages ({})",
                        db_path.display(),
                        sessions,
                        messages,
                        size_str
                    );
                } else {
                    println!("  Database:  {} ({})", db_path.display(), size_str);
                }
            }
            Err(_) => println!("  Database:  {} ({})", db_path.display(), size_str),
        }
    } else {
        println!("  Database:  not initialized");
    }

    // Channels
    let mut channels = Vec::new();
    if config.channels.telegram.enabled {
        channels.push("Telegram");
    }
    if config.channels.discord.enabled {
        channels.push("Discord");
    }
    if config.channels.slack.enabled {
        channels.push("Slack");
    }
    if config.channels.whatsapp.enabled {
        channels.push("WhatsApp");
    }
    if config.channels.trello.enabled {
        channels.push("Trello");
    }
    if channels.is_empty() {
        println!("  Channels:  none enabled");
    } else {
        println!("  Channels:  {}", channels.join(", "));
    }

    // A2A
    if config.a2a.enabled {
        println!(
            "  A2A:       enabled ({}:{})",
            config.a2a.bind, config.a2a.port
        );
    }

    // Dynamic tools
    let tools_path = BrainLoader::resolve_path().join("tools.toml");
    if tools_path.exists()
        && let Ok(contents) = std::fs::read_to_string(&tools_path)
    {
        let count = contents.matches("[[tools]]").count();
        if count > 0 {
            println!("  Tools:     {} dynamic tool(s) in tools.toml", count);
        }
    }

    // Cron
    let cron_path = BrainLoader::resolve_path().join("cron.toml");
    if cron_path.exists()
        && let Ok(contents) = std::fs::read_to_string(&cron_path)
    {
        let count = contents.matches("[[jobs]]").count();
        if count > 0 {
            println!("  Cron:      {} job(s)", count);
        }
    }

    // Logs
    let log_dir = dirs::config_dir()
        .map(|d| d.join("opencrabs").join("logs"))
        .unwrap_or_default();
    if log_dir.exists() {
        let log_count = std::fs::read_dir(&log_dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter(|e| {
                        e.path()
                            .extension()
                            .map(|ext| ext == "log")
                            .unwrap_or(false)
                    })
                    .count()
            })
            .unwrap_or(0);
        if log_count > 0 {
            println!(
                "  Logs:      {} file(s) in {}",
                log_count,
                log_dir.display()
            );
        }
    }

    println!();
    Ok(())
}

/// Run diagnostics
pub(crate) async fn cmd_doctor(config: &crate::config::Config) -> Result<()> {
    use crate::db::Database;

    let version = env!("CARGO_PKG_VERSION");
    println!("🦀 OpenCrabs Doctor v{version}\n");

    let mut pass = 0u32;
    let mut fail = 0u32;
    let mut warn = 0u32;

    // 1. Config file
    let config_path = crate::config::Config::system_config_path();
    if let Some(ref p) = config_path
        && p.exists()
    {
        println!("  ✅ Config file: {}", p.display());
        pass += 1;
    } else {
        println!("  ❌ Config file: not found (run `opencrabs init` or `opencrabs onboard`)");
        fail += 1;
    }

    // 2. Keys file
    let keys_path = crate::config::keys_path();
    if keys_path.exists() {
        println!("  ✅ Keys file: {}", keys_path.display());
        pass += 1;
    } else {
        println!("  ⚠️  Keys file: not found (API keys stored in config.toml or env vars)");
        warn += 1;
    }

    // 3. Provider
    match crate::brain::provider::create_provider(config).await {
        Ok(provider) => {
            println!(
                "  ✅ Provider: {} (model: {})",
                provider.name(),
                provider.default_model()
            );
            pass += 1;
        }
        Err(e) => {
            println!("  ❌ Provider: {}", e);
            fail += 1;
        }
    }

    // 4. Database
    let db_path = &config.database.path;
    if db_path.exists() {
        match Database::connect(db_path).await {
            Ok(db) => {
                db.run_migrations().await.ok();
                println!("  ✅ Database: {}", db_path.display());
                pass += 1;
            }
            Err(e) => {
                println!("  ❌ Database: failed to connect — {}", e);
                fail += 1;
            }
        }
    } else {
        println!(
            "  ❌ Database: not found at {} (run `opencrabs db init`)",
            db_path.display()
        );
        fail += 1;
    }

    // 5. Brain files
    let brain_path = BrainLoader::resolve_path();
    if brain_path.exists() {
        let brain_files: Vec<&str> = ["persona.md", "system.md", "USER.md", "MEMORY.md"]
            .iter()
            .filter(|f| brain_path.join(f).exists())
            .copied()
            .collect();
        if brain_files.is_empty() {
            println!(
                "  ⚠️  Brain: directory exists but no brain files found at {}",
                brain_path.display()
            );
            warn += 1;
        } else {
            println!(
                "  ✅ Brain: {} files ({})",
                brain_files.len(),
                brain_files.join(", ")
            );
            pass += 1;
        }
    } else {
        println!(
            "  ⚠️  Brain: directory not found at {}",
            brain_path.display()
        );
        warn += 1;
    }

    // 6. Channels
    println!();
    println!("  Channels:");

    // Telegram
    if config.channels.telegram.enabled {
        if config
            .channels
            .telegram
            .token
            .as_ref()
            .is_some_and(|t| !t.is_empty())
        {
            println!("    ✅ Telegram: enabled, token set");
            pass += 1;
        } else {
            println!("    ❌ Telegram: enabled but no bot token");
            fail += 1;
        }
    } else {
        println!("    ⬚  Telegram: disabled");
    }

    // Discord
    if config.channels.discord.enabled {
        if config
            .channels
            .discord
            .token
            .as_ref()
            .is_some_and(|t| !t.is_empty())
        {
            println!("    ✅ Discord: enabled, token set");
            pass += 1;
        } else {
            println!("    ❌ Discord: enabled but no bot token");
            fail += 1;
        }
    } else {
        println!("    ⬚  Discord: disabled");
    }

    // Slack
    if config.channels.slack.enabled {
        let has_bot = config
            .channels
            .slack
            .token
            .as_ref()
            .is_some_and(|t| !t.is_empty());
        let has_app = config
            .channels
            .slack
            .app_token
            .as_ref()
            .is_some_and(|t| !t.is_empty());
        if has_bot && has_app {
            println!("    ✅ Slack: enabled, bot + app tokens set");
            pass += 1;
        } else {
            let missing: Vec<&str> = [
                (!has_bot).then_some("bot token"),
                (!has_app).then_some("app token"),
            ]
            .into_iter()
            .flatten()
            .collect();
            println!("    ❌ Slack: enabled but missing {}", missing.join(", "));
            fail += 1;
        }
    } else {
        println!("    ⬚  Slack: disabled");
    }

    // WhatsApp
    if config.channels.whatsapp.enabled {
        println!("    ✅ WhatsApp: enabled (pairs at runtime)");
        pass += 1;
    } else {
        println!("    ⬚  WhatsApp: disabled");
    }

    // Trello
    if config.channels.trello.enabled {
        let has_token = config
            .channels
            .trello
            .token
            .as_ref()
            .is_some_and(|t| !t.is_empty());
        let has_app_token = config
            .channels
            .trello
            .app_token
            .as_ref()
            .is_some_and(|t| !t.is_empty());
        if has_token && has_app_token {
            println!("    ✅ Trello: enabled, token + app token set");
            pass += 1;
        } else {
            println!("    ❌ Trello: enabled but missing credentials");
            fail += 1;
        }
    } else {
        println!("    ⬚  Trello: disabled");
    }

    // 7. CLI tools in PATH
    println!();
    println!("  CLI tools:");
    for (name, desc) in [
        ("claude", "Claude CLI provider"),
        ("opencode", "OpenCode CLI provider"),
        ("codex", "Codex CLI provider"),
        ("docker", "container runtime"),
        ("ffmpeg", "media processing"),
        ("gh", "GitHub CLI"),
    ] {
        if which::which(name).is_ok() {
            println!("    ✅ {name}: found ({desc})");
            pass += 1;
        } else {
            println!("    ⬚  {name}: not found ({desc})");
        }
    }

    // 8. Dynamic tools
    let tools_path = brain_path.join("tools.toml");
    if tools_path.exists()
        && let Ok(contents) = std::fs::read_to_string(&tools_path)
    {
        let count = contents.matches("[[tools]]").count();
        if count > 0 {
            println!();
            println!("  ✅ Dynamic tools: {} defined in tools.toml", count);
            pass += 1;
        }
    }

    // Summary
    println!();
    if fail == 0 {
        println!("  ✅ All checks passed ({pass} ok, {warn} warnings)");
    } else {
        println!("  {fail} issue(s), {pass} ok, {warn} warning(s)");
    }
    println!();

    Ok(())
}

/// Load configuration from file or defaults
pub(crate) async fn load_config(config_path: Option<&str>) -> Result<crate::config::Config> {
    use crate::config::Config;

    let config = if let Some(path) = config_path {
        tracing::info!("Loading configuration from custom path: {}", path);
        Config::load_from_path(path)?
    } else {
        tracing::debug!("Loading default configuration");
        Config::load()?
    };

    // Validate configuration
    config.validate()?;

    Ok(config)
}

/// Initialize configuration file
pub(crate) async fn cmd_init(_config: &crate::config::Config, force: bool) -> Result<()> {
    use crate::config::Config;

    println!("🦀 OpenCrabs Configuration Initialization\n");

    let config_path = Config::system_config_path()
        .unwrap_or_else(|| crate::config::opencrabs_home().join("config.toml"));

    // Check if config already exists
    if config_path.exists() && !force {
        anyhow::bail!(
            "Configuration file already exists at: {}\nUse --force to overwrite",
            config_path.display()
        );
    }

    // Save default configuration
    let default_config = Config::default();
    default_config.save(&config_path)?;

    println!("✅ Configuration initialized at: {}", config_path.display());
    println!("\n📝 Next steps:");
    println!("   1. Edit the config file to add your API keys");
    println!("   2. Set ANTHROPIC_API_KEY environment variable");
    println!("   3. Run 'opencrabs' or 'opencrabs chat' to start");

    Ok(())
}

/// Show configuration
pub(crate) async fn cmd_config(config: &crate::config::Config, show_secrets: bool) -> Result<()> {
    println!("🦀 OpenCrabs Configuration\n");

    if show_secrets {
        println!("{:#?}", config);
    } else {
        println!("Database: {}", config.database.path.display());
        println!("Log level: {}", config.logging.level);
        println!("\nProviders:");

        if let Some(ref anthropic) = config.providers.anthropic {
            println!(
                "  - anthropic: {}",
                anthropic
                    .default_model
                    .as_ref()
                    .unwrap_or(&"claude-3-5-sonnet-20240620".to_string())
            );
            println!(
                "    API Key: {}",
                if anthropic.api_key.is_some() {
                    "[SET]"
                } else {
                    "[NOT SET]"
                }
            );
        }

        if let Some(ref openai) = config.providers.openai {
            println!(
                "  - openai: {}",
                openai
                    .default_model
                    .as_ref()
                    .unwrap_or(&"gpt-4".to_string())
            );
            println!(
                "    API Key: {}",
                if openai.api_key.is_some() {
                    "[SET]"
                } else {
                    "[NOT SET]"
                }
            );
        }

        println!("\n💡 Use --show-secrets to display API keys");
    }

    Ok(())
}

/// Database operations
pub(crate) async fn cmd_db(config: &crate::config::Config, operation: DbCommands) -> Result<()> {
    use crate::db::Database;

    match operation {
        DbCommands::Init => {
            println!("🗄️  Initializing database...");
            let db = Database::connect(&config.database.path).await?;
            db.run_migrations().await?;
            println!(
                "✅ Database initialized at: {}",
                config.database.path.display()
            );
            Ok(())
        }
        DbCommands::Stats => {
            println!("📊 Database Statistics\n");
            let db = Database::connect(&config.database.path).await?;

            let (session_count, message_count, file_count) = db
                .pool()
                .get()
                .await
                .context("Failed to get connection")?
                .interact(|conn| {
                    let sessions: i64 =
                        conn.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))?;
                    let messages: i64 =
                        conn.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))?;
                    let files: i64 =
                        conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
                    Ok::<_, rusqlite::Error>((sessions, messages, files))
                })
                .await
                .map_err(crate::db::interact_err)?
                .context("Failed to query stats")?;

            println!("Sessions: {}", session_count);
            println!("Messages: {}", message_count);
            println!("Files: {}", file_count);

            Ok(())
        }
        DbCommands::Clear { force } => {
            let db = Database::connect(&config.database.path).await?;

            let (session_count, message_count, file_count) = db
                .pool()
                .get()
                .await
                .context("Failed to get connection")?
                .interact(|conn| {
                    let sessions: i64 =
                        conn.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))?;
                    let messages: i64 =
                        conn.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))?;
                    let files: i64 =
                        conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
                    Ok::<_, rusqlite::Error>((sessions, messages, files))
                })
                .await
                .map_err(crate::db::interact_err)?
                .context("Failed to query counts")?;

            if session_count == 0 && message_count == 0 && file_count == 0 {
                println!("✨ Database is already empty");
                return Ok(());
            }

            println!("⚠️  WARNING: This will permanently delete ALL data:\n");
            println!("   • {} sessions", session_count);
            println!("   • {} messages", message_count);
            println!("   • {} files", file_count);
            println!();

            // Confirmation prompt
            if !force {
                use std::io::{self, Write};
                print!("Type 'yes' to confirm deletion: ");
                io::stdout().flush()?;

                let mut input = String::new();
                io::stdin().read_line(&mut input)?;

                if input.trim().to_lowercase() != "yes" {
                    println!("❌ Cancelled - no data was deleted");
                    return Ok(());
                }
            }

            // Clear all tables
            println!("\n🗑️  Clearing database...");

            // Delete in correct order to respect foreign key constraints
            db.pool()
                .get()
                .await
                .context("Failed to get connection")?
                .interact(|conn| {
                    conn.execute("DELETE FROM messages", [])?;
                    conn.execute("DELETE FROM files", [])?;
                    conn.execute("DELETE FROM sessions", [])?;
                    Ok::<_, rusqlite::Error>(())
                })
                .await
                .map_err(crate::db::interact_err)?
                .context("Failed to clear database")?;

            println!(
                "✅ Successfully cleared {} sessions, {} messages, and {} files",
                session_count, message_count, file_count
            );

            Ok(())
        }
    }
}

/// Run a single command non-interactively
pub(crate) async fn cmd_run(
    config: &crate::config::Config,
    prompt: String,
    auto_approve: bool,
    format: OutputFormat,
) -> Result<()> {
    use crate::{
        brain::{
            agent::AgentService,
            tools::{
                bash::BashTool, brave_search::BraveSearchTool, code_exec::CodeExecTool,
                config_tool::ConfigTool, context::ContextTool, doc_parser::DocParserTool,
                edit::EditTool, exa_search::ExaSearchTool,
                follow_up_question::FollowUpQuestionTool, glob::GlobTool, grep::GrepTool,
                http::HttpClientTool, ls::LsTool, memory_search::MemorySearchTool,
                notebook::NotebookEditTool, pdf_to_images::PdfToImagesTool, plan_tool::PlanTool,
                read::ReadTool, registry::ToolRegistry, rename_session::RenameSessionTool,
                session_search::SessionSearchTool, slash_command::SlashCommandTool, task::TaskTool,
                web_search::WebSearchTool, write::WriteTool,
            },
        },
        db::Database,
        services::{ServiceContext, SessionService},
    };

    tracing::info!("Running non-interactive command: {}", prompt);

    // Initialize database
    let db = Database::connect(&config.database.path).await?;
    db.run_migrations().await?;

    // Select provider based on configuration using factory
    let provider = crate::brain::provider::create_provider(config).await?;

    // Create tool registry (Arc-wrapped early so SpawnAgentTool can reference it)
    let tool_registry = Arc::new(ToolRegistry::new());
    // Phase 1: Essential file operations
    tool_registry.register(Arc::new(ReadTool));
    tool_registry.register(Arc::new(WriteTool));
    tool_registry.register(Arc::new(EditTool));
    tool_registry.register(Arc::new(crate::brain::tools::hashline::HashlineEditTool));
    tool_registry.register(Arc::new(BashTool));
    tool_registry.register(Arc::new(LsTool));
    tool_registry.register(Arc::new(GlobTool));
    tool_registry.register(Arc::new(GrepTool));
    // Phase 2: Advanced features
    tool_registry.register(Arc::new(WebSearchTool));
    tool_registry.register(Arc::new(CodeExecTool));
    tool_registry.register(Arc::new(NotebookEditTool));
    tool_registry.register(Arc::new(DocParserTool));
    tool_registry.register(Arc::new(PdfToImagesTool));
    // Phase 3: Workflow & integration
    tool_registry.register(Arc::new(TaskTool));
    tool_registry.register(Arc::new(ContextTool));
    tool_registry.register(Arc::new(HttpClientTool));
    tool_registry.register(Arc::new(PlanTool));
    // Memory search (built-in FTS5, always available)
    tool_registry.register(Arc::new(MemorySearchTool));
    // Session search — hybrid QMD search across all session message history
    tool_registry.register(Arc::new(SessionSearchTool::new(db.pool().clone())));
    tool_registry.register(Arc::new(
        crate::brain::tools::mission_control_report::MissionControlReportTool::new(
            db.pool().clone(),
        ),
    ));
    // Config management (read/write config.toml, commands.toml)
    tool_registry.register(Arc::new(ConfigTool));
    // Slash command invocation (agent can call any slash command)
    tool_registry.register(Arc::new(SlashCommandTool));
    // Session rename — agent can update the current session's title
    tool_registry.register(Arc::new(RenameSessionTool));
    // Follow-up question — agent asks the user a multi-choice question
    // mid-task and blocks until they click an option button.
    tool_registry.register(Arc::new(FollowUpQuestionTool));
    // EXA search: always available (free via MCP), uses direct API if key is set
    let exa_key = config
        .providers
        .web_search
        .as_ref()
        .and_then(|ws| ws.exa.as_ref())
        .and_then(|p| p.api_key.clone())
        .filter(|k| !k.is_empty());
    tool_registry.register(Arc::new(ExaSearchTool::new(exa_key)));
    // Brave search: requires enabled = true in config.toml AND API key in keys.toml
    if let Some(brave_cfg) = config
        .providers
        .web_search
        .as_ref()
        .and_then(|ws| ws.brave.as_ref())
        && brave_cfg.enabled
        && let Some(brave_key) = brave_cfg.api_key.clone()
    {
        tool_registry.register(Arc::new(BraveSearchTool::new(brave_key)));
    }

    // Phase 5: Multi-agent orchestration
    let subagent_manager = Arc::new(crate::brain::tools::subagent::SubAgentManager::new());
    tool_registry.register(Arc::new(
        crate::brain::tools::subagent::SpawnAgentTool::new(
            subagent_manager.clone(),
            tool_registry.clone(),
        ),
    ));
    tool_registry.register(Arc::new(crate::brain::tools::subagent::WaitAgentTool::new(
        subagent_manager.clone(),
    )));
    tool_registry.register(Arc::new(crate::brain::tools::subagent::SendInputTool::new(
        subagent_manager.clone(),
    )));
    tool_registry.register(Arc::new(
        crate::brain::tools::subagent::CloseAgentTool::new(subagent_manager.clone()),
    ));
    tool_registry.register(Arc::new(
        crate::brain::tools::subagent::ResumeAgentTool::new(
            subagent_manager.clone(),
            tool_registry.clone(),
        ),
    ));

    let team_manager = Arc::new(crate::brain::tools::subagent::TeamManager::new());
    tool_registry.register(Arc::new(
        crate::brain::tools::subagent::TeamCreateTool::new(
            subagent_manager.clone(),
            team_manager.clone(),
            tool_registry.clone(),
        ),
    ));
    tool_registry.register(Arc::new(
        crate::brain::tools::subagent::TeamDeleteTool::new(
            subagent_manager.clone(),
            team_manager.clone(),
        ),
    ));
    tool_registry.register(Arc::new(
        crate::brain::tools::subagent::TeamBroadcastTool::new(
            subagent_manager.clone(),
            team_manager.clone(),
        ),
    ));

    // Recursive Self-Improvement tools
    use crate::brain::tools::feedback_analyze::FeedbackAnalyzeTool;
    use crate::brain::tools::feedback_record::FeedbackRecordTool;
    use crate::brain::tools::self_improve::SelfImproveTool;
    tool_registry.register(Arc::new(FeedbackRecordTool));
    tool_registry.register(Arc::new(FeedbackAnalyzeTool));
    tool_registry.register(Arc::new(SelfImproveTool));

    // Build dynamic system brain from workspace files
    let brain_path = BrainLoader::resolve_path();
    let brain_loader = BrainLoader::new(brain_path.clone());
    let runtime_info = RuntimeInfo {
        model: Some(provider.default_model().to_string()),
        provider: Some(provider.name().to_string()),
        // Collapse $HOME → ~ for the same reason as the TUI runtime
        // info builder: don't leak username, save tokens, keep cache
        // keys stable across machines.
        working_directory: Some(crate::brain::tools::error::collapse_home(
            &std::env::current_dir().unwrap_or_default(),
        )),
    };
    // Tool discovery for lazy-tools mode (harmless when off).
    tool_registry.register(Arc::new(
        crate::brain::tools::tool_search::ToolSearchTool::new(tool_registry.clone()),
    ));
    // Feedback/performance digest stays out of the LLM context — it's a
    // maintenance warning in ~/.opencrabs/rsi/digest.md, not conversation input.
    let mut system_brain = brain_loader.build_system_brain(Some(&runtime_info), None);
    if config.agent.lazy_tools {
        system_brain.push_str(crate::brain::tools::catalog::LAZY_TOOLS_PROMPT);
    }

    // Load dynamic tools from ~/.opencrabs/tools.toml
    let tools_toml_path = crate::brain::tools::dynamic::DynamicToolLoader::default_path()
        .unwrap_or_else(|| std::path::PathBuf::from("tools.toml"));
    let dynamic_count =
        crate::brain::tools::dynamic::DynamicToolLoader::load(&tools_toml_path, &tool_registry);
    if dynamic_count > 0 {
        tracing::info!("Loaded {dynamic_count} dynamic tool(s) from tools.toml");
    }

    // Create service context and agent service
    let service_context = ServiceContext::new(db.pool().clone());
    let agent_service = AgentService::new(provider.clone(), service_context.clone(), config)
        .await
        .with_tool_registry(tool_registry.clone())
        .with_system_brain(system_brain);

    // Create or get session
    let session_service = SessionService::new(service_context);

    let session = session_service
        .create_session(Some("CLI Run".to_string()))
        .await?;

    // Send message
    println!("🤔 Processing...\n");
    let response = agent_service.send_message(session.id, prompt, None).await?;

    // Format and display output
    match format {
        OutputFormat::Text => {
            println!("{}", response.content);
            println!();
            println!(
                "📊 Tokens: {}",
                response.usage.input_tokens + response.usage.output_tokens
            );
            println!("💰 Cost: ${:.6}", response.cost);
        }
        OutputFormat::Json => {
            let output = serde_json::json!({
                "content": response.content,
                "usage": {
                    "input_tokens": response.usage.input_tokens,
                    "output_tokens": response.usage.output_tokens,
                },
                "cost": response.cost,
                "model": response.model,
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        OutputFormat::Markdown => {
            println!("# Response\n");
            println!("{}\n", response.content);
            println!("---");
            println!(
                "**Tokens:** {}",
                response.usage.input_tokens + response.usage.output_tokens
            );
            println!("**Cost:** ${:.6}", response.cost);
        }
    }

    if auto_approve {
        println!("\n⚠️  Auto-approve mode was enabled");
    }

    Ok(())
}

/// Log management commands
pub(crate) async fn cmd_logs(operation: LogCommands) -> Result<()> {
    use crate::logging;
    use std::io::{BufRead, BufReader};

    // Resolve the SAME directory the writer uses (~/.opencrabs/logs or the
    // DEBUG_LOGS_LOCATION override). The old code looked under the process's
    // CWD, so a daemon (whose working dir isn't home) always saw an empty
    // directory and reported "Log files: 0" (#190 secondary).
    let log_dir = logging::log_dir();

    match operation {
        LogCommands::Status => {
            println!("📊 OpenCrabs Logging Status\n");
            println!("Log directory: {}", log_dir.display());

            if log_dir.exists() {
                // Count log files and total size
                let mut file_count = 0;
                let mut total_size = 0u64;
                let mut newest_file: Option<std::path::PathBuf> = None;
                let mut newest_time = std::time::UNIX_EPOCH;

                for entry in std::fs::read_dir(&log_dir)? {
                    let entry = entry?;
                    let path = entry.path();
                    if entry.file_name().to_str().is_some_and(logging::is_log_file) {
                        file_count += 1;
                        if let Ok(metadata) = entry.metadata() {
                            total_size += metadata.len();
                            if let Ok(modified) = metadata.modified()
                                && modified > newest_time
                            {
                                newest_time = modified;
                                newest_file = Some(path);
                            }
                        }
                    }
                }

                println!("Status: ✅ Active");
                println!("Log files: {}", file_count);
                println!(
                    "Total size: {:.2} MB",
                    total_size as f64 / (1024.0 * 1024.0)
                );

                if let Some(newest) = newest_file {
                    println!("Latest log: {}", newest.display());
                }

                println!("\n💡 To enable debug logging, run with -d flag:");
                println!("   opencrabs -d");
            } else {
                println!("Status: ❌ No logs found");
                println!("\n💡 To enable debug logging, run with -d flag:");
                println!("   opencrabs -d");
                println!("\nThis will create log files in:");
                println!("   {}", log_dir.display());
            }

            Ok(())
        }

        LogCommands::View { lines } => {
            if let Some(log_path) = logging::get_log_path() {
                println!(
                    "📜 Viewing last {} lines of: {}\n",
                    lines,
                    log_path.display()
                );

                let file = std::fs::File::open(&log_path)?;
                let reader = BufReader::new(file);

                // Collect all lines then show last N
                let all_lines: Vec<String> = reader.lines().map_while(Result::ok).collect();
                let start = all_lines.len().saturating_sub(lines);

                for line in &all_lines[start..] {
                    println!("{}", line);
                }

                if all_lines.is_empty() {
                    println!("(empty log file)");
                }
            } else {
                println!("❌ No log files found.\n");
                println!("💡 Run OpenCrabs with -d flag to enable debug logging:");
                println!("   opencrabs -d");
            }

            Ok(())
        }

        LogCommands::Clean { days } => {
            println!("🧹 Cleaning up log files older than {} days...\n", days);

            match logging::cleanup_old_logs(days) {
                Ok(removed) => {
                    if removed > 0 {
                        println!("✅ Removed {} old log file(s)", removed);
                    } else {
                        println!("✅ No old log files to remove");
                    }
                }
                Err(e) => {
                    println!("❌ Error cleaning logs: {}", e);
                }
            }

            Ok(())
        }

        LogCommands::Open => {
            if !log_dir.exists() {
                println!("❌ Log directory does not exist: {}", log_dir.display());
                println!("\n💡 Run OpenCrabs with -d flag to enable debug logging:");
                println!("   opencrabs -d");
                return Ok(());
            }

            println!("📂 Opening log directory: {}", log_dir.display());

            #[cfg(target_os = "macos")]
            {
                std::process::Command::new("open")
                    .arg(&log_dir)
                    .spawn()
                    .context("Failed to open directory")?;
            }

            #[cfg(target_os = "linux")]
            {
                std::process::Command::new("xdg-open")
                    .arg(&log_dir)
                    .spawn()
                    .context("Failed to open directory")?;
            }

            #[cfg(target_os = "windows")]
            {
                std::process::Command::new("explorer")
                    .arg(&log_dir)
                    .spawn()
                    .context("Failed to open directory")?;
            }

            Ok(())
        }
    }
}

/// Interactive CLI agent — multi-turn conversation without TUI
pub(crate) async fn cmd_agent_interactive(
    config: &crate::config::Config,
    auto_approve: bool,
) -> Result<()> {
    use crate::{
        brain::{
            agent::AgentService,
            tools::{
                bash::BashTool, brave_search::BraveSearchTool, code_exec::CodeExecTool,
                config_tool::ConfigTool, context::ContextTool, doc_parser::DocParserTool,
                edit::EditTool, exa_search::ExaSearchTool,
                follow_up_question::FollowUpQuestionTool, glob::GlobTool, grep::GrepTool,
                http::HttpClientTool, ls::LsTool, memory_search::MemorySearchTool,
                notebook::NotebookEditTool, pdf_to_images::PdfToImagesTool, plan_tool::PlanTool,
                read::ReadTool, registry::ToolRegistry, rename_session::RenameSessionTool,
                session_search::SessionSearchTool, slash_command::SlashCommandTool, task::TaskTool,
                web_search::WebSearchTool, write::WriteTool,
            },
        },
        db::Database,
        services::{ServiceContext, SessionService},
    };
    use std::io::{self, BufRead, Write};

    let _ = auto_approve; // TODO: wire into approval callback

    let db = Database::connect(&config.database.path).await?;
    db.run_migrations().await?;

    let provider = crate::brain::provider::create_provider(config).await?;

    let tool_registry = Arc::new(ToolRegistry::new());
    tool_registry.register(Arc::new(ReadTool));
    tool_registry.register(Arc::new(WriteTool));
    tool_registry.register(Arc::new(EditTool));
    tool_registry.register(Arc::new(BashTool));
    tool_registry.register(Arc::new(LsTool));
    tool_registry.register(Arc::new(GlobTool));
    tool_registry.register(Arc::new(GrepTool));
    tool_registry.register(Arc::new(WebSearchTool));
    tool_registry.register(Arc::new(CodeExecTool));
    tool_registry.register(Arc::new(NotebookEditTool));
    tool_registry.register(Arc::new(DocParserTool));
    tool_registry.register(Arc::new(PdfToImagesTool));
    tool_registry.register(Arc::new(TaskTool));
    tool_registry.register(Arc::new(ContextTool));
    tool_registry.register(Arc::new(HttpClientTool));
    tool_registry.register(Arc::new(PlanTool));
    tool_registry.register(Arc::new(MemorySearchTool));
    tool_registry.register(Arc::new(SessionSearchTool::new(db.pool().clone())));
    tool_registry.register(Arc::new(
        crate::brain::tools::mission_control_report::MissionControlReportTool::new(
            db.pool().clone(),
        ),
    ));
    tool_registry.register(Arc::new(ConfigTool));
    tool_registry.register(Arc::new(SlashCommandTool));
    tool_registry.register(Arc::new(RenameSessionTool));
    // Follow-up question — agent asks the user a multi-choice question
    // mid-task and blocks until they click an option button.
    tool_registry.register(Arc::new(FollowUpQuestionTool));
    let exa_key = config
        .providers
        .web_search
        .as_ref()
        .and_then(|ws| ws.exa.as_ref())
        .and_then(|p| p.api_key.clone())
        .filter(|k| !k.is_empty());
    tool_registry.register(Arc::new(ExaSearchTool::new(exa_key)));
    if let Some(brave_cfg) = config
        .providers
        .web_search
        .as_ref()
        .and_then(|ws| ws.brave.as_ref())
        && brave_cfg.enabled
        && let Some(brave_key) = brave_cfg.api_key.clone()
    {
        tool_registry.register(Arc::new(BraveSearchTool::new(brave_key)));
    }

    let subagent_manager = Arc::new(crate::brain::tools::subagent::SubAgentManager::new());
    tool_registry.register(Arc::new(
        crate::brain::tools::subagent::SpawnAgentTool::new(
            subagent_manager.clone(),
            tool_registry.clone(),
        ),
    ));
    tool_registry.register(Arc::new(crate::brain::tools::subagent::WaitAgentTool::new(
        subagent_manager.clone(),
    )));
    tool_registry.register(Arc::new(crate::brain::tools::subagent::SendInputTool::new(
        subagent_manager.clone(),
    )));
    tool_registry.register(Arc::new(
        crate::brain::tools::subagent::CloseAgentTool::new(subagent_manager.clone()),
    ));
    tool_registry.register(Arc::new(
        crate::brain::tools::subagent::ResumeAgentTool::new(
            subagent_manager.clone(),
            tool_registry.clone(),
        ),
    ));

    let team_manager = Arc::new(crate::brain::tools::subagent::TeamManager::new());
    tool_registry.register(Arc::new(
        crate::brain::tools::subagent::TeamCreateTool::new(
            subagent_manager.clone(),
            team_manager.clone(),
            tool_registry.clone(),
        ),
    ));
    tool_registry.register(Arc::new(
        crate::brain::tools::subagent::TeamDeleteTool::new(
            subagent_manager.clone(),
            team_manager.clone(),
        ),
    ));
    tool_registry.register(Arc::new(
        crate::brain::tools::subagent::TeamBroadcastTool::new(
            subagent_manager.clone(),
            team_manager.clone(),
        ),
    ));

    // Recursive Self-Improvement tools
    tool_registry.register(Arc::new(
        crate::brain::tools::feedback_record::FeedbackRecordTool,
    ));
    tool_registry.register(Arc::new(
        crate::brain::tools::feedback_analyze::FeedbackAnalyzeTool,
    ));
    tool_registry.register(Arc::new(crate::brain::tools::self_improve::SelfImproveTool));

    let brain_path = BrainLoader::resolve_path();
    let brain_loader = BrainLoader::new(brain_path);
    let runtime_info = RuntimeInfo {
        model: Some(provider.default_model().to_string()),
        provider: Some(provider.name().to_string()),
        working_directory: Some(crate::brain::tools::error::collapse_home(
            &std::env::current_dir().unwrap_or_default(),
        )),
    };
    // Tool discovery for lazy-tools mode (harmless when off).
    tool_registry.register(Arc::new(
        crate::brain::tools::tool_search::ToolSearchTool::new(tool_registry.clone()),
    ));
    // Feedback/performance digest stays out of the LLM context — it's a
    // maintenance warning in ~/.opencrabs/rsi/digest.md, not conversation input.
    let mut system_brain = brain_loader.build_system_brain(Some(&runtime_info), None);
    if config.agent.lazy_tools {
        system_brain.push_str(crate::brain::tools::catalog::LAZY_TOOLS_PROMPT);
    }

    // Load dynamic tools from ~/.opencrabs/tools.toml
    let tools_toml_path = crate::brain::tools::dynamic::DynamicToolLoader::default_path()
        .unwrap_or_else(|| std::path::PathBuf::from("tools.toml"));
    let dynamic_count =
        crate::brain::tools::dynamic::DynamicToolLoader::load(&tools_toml_path, &tool_registry);
    if dynamic_count > 0 {
        tracing::info!("Loaded {dynamic_count} dynamic tool(s) from tools.toml");
    }

    let service_context = ServiceContext::new(db.pool().clone());
    let agent_service = AgentService::new(provider.clone(), service_context.clone(), config)
        .await
        .with_tool_registry(tool_registry.clone())
        .with_system_brain(system_brain);

    let session_service = SessionService::new(service_context);
    let session = session_service
        .create_session(Some("CLI Agent".to_string()))
        .await?;

    println!(
        "🦀 OpenCrabs Agent — {} ({})",
        provider.name(),
        provider.default_model()
    );
    println!("Type /exit or Ctrl+D to quit\n");

    let stdin = io::stdin();
    let mut reader = stdin.lock();

    loop {
        print!("❯ ");
        io::stdout().flush()?;

        let mut input = String::new();
        if reader.read_line(&mut input)? == 0 {
            println!();
            break;
        }

        let input = input.trim();
        if input.is_empty() {
            continue;
        }
        if input == "/exit" || input == "/quit" || input == "/q" {
            break;
        }

        match agent_service
            .send_message(session.id, input.to_string(), None)
            .await
        {
            Ok(response) => {
                println!("\n{}\n", response.content);
                println!(
                    "  tokens: {} | cost: ${:.6}\n",
                    response.usage.input_tokens + response.usage.output_tokens,
                    response.cost
                );
            }
            Err(e) => {
                eprintln!("\n  error: {e}\n");
            }
        }
    }

    Ok(())
}

/// Channel operations
pub(crate) async fn cmd_channel(
    config: &crate::config::Config,
    operation: ChannelCommands,
) -> Result<()> {
    match operation {
        ChannelCommands::List => {
            println!("🦀 Configured Channels\n");

            let channels: Vec<(&str, bool, &str)> = vec![
                (
                    "Telegram",
                    config.channels.telegram.enabled,
                    if config
                        .channels
                        .telegram
                        .token
                        .as_ref()
                        .is_some_and(|t| !t.is_empty())
                    {
                        "token set"
                    } else {
                        "no token"
                    },
                ),
                (
                    "Discord",
                    config.channels.discord.enabled,
                    if config
                        .channels
                        .discord
                        .token
                        .as_ref()
                        .is_some_and(|t| !t.is_empty())
                    {
                        "token set"
                    } else {
                        "no token"
                    },
                ),
                (
                    "Slack",
                    config.channels.slack.enabled,
                    if config
                        .channels
                        .slack
                        .token
                        .as_ref()
                        .is_some_and(|t| !t.is_empty())
                        && config
                            .channels
                            .slack
                            .app_token
                            .as_ref()
                            .is_some_and(|t| !t.is_empty())
                    {
                        "bot + app tokens set"
                    } else {
                        "missing tokens"
                    },
                ),
                (
                    "WhatsApp",
                    config.channels.whatsapp.enabled,
                    "pairs at runtime",
                ),
                (
                    "Trello",
                    config.channels.trello.enabled,
                    if config
                        .channels
                        .trello
                        .token
                        .as_ref()
                        .is_some_and(|t| !t.is_empty())
                    {
                        "token set"
                    } else {
                        "no token"
                    },
                ),
            ];

            for (name, enabled, detail) in channels {
                let status = if enabled { "✅" } else { "⬚ " };
                let state = if enabled { "enabled" } else { "disabled" };
                println!("  {status} {name:<12} {state:<10} ({detail})");
            }
            println!();
            Ok(())
        }
        ChannelCommands::Doctor => {
            println!("🦀 Channel Health Check\n");
            cmd_doctor_channels(config);
            println!();
            Ok(())
        }
    }
}

/// Shared channel diagnostics
fn cmd_doctor_channels(config: &crate::config::Config) {
    if config.channels.telegram.enabled {
        if config
            .channels
            .telegram
            .token
            .as_ref()
            .is_some_and(|t| !t.is_empty())
        {
            println!("  ✅ Telegram: enabled, token set");
        } else {
            println!("  ❌ Telegram: enabled but no bot token");
        }
    } else {
        println!("  ⬚  Telegram: disabled");
    }

    if config.channels.discord.enabled {
        if config
            .channels
            .discord
            .token
            .as_ref()
            .is_some_and(|t| !t.is_empty())
        {
            println!("  ✅ Discord: enabled, token set");
        } else {
            println!("  ❌ Discord: enabled but no bot token");
        }
    } else {
        println!("  ⬚  Discord: disabled");
    }

    if config.channels.slack.enabled {
        let has_bot = config
            .channels
            .slack
            .token
            .as_ref()
            .is_some_and(|t| !t.is_empty());
        let has_app = config
            .channels
            .slack
            .app_token
            .as_ref()
            .is_some_and(|t| !t.is_empty());
        if has_bot && has_app {
            println!("  ✅ Slack: enabled, bot + app tokens set");
        } else {
            let missing: Vec<&str> = [
                (!has_bot).then_some("bot token"),
                (!has_app).then_some("app token"),
            ]
            .into_iter()
            .flatten()
            .collect();
            println!("  ❌ Slack: enabled but missing {}", missing.join(", "));
        }
    } else {
        println!("  ⬚  Slack: disabled");
    }

    if config.channels.whatsapp.enabled {
        println!("  ✅ WhatsApp: enabled (pairs at runtime)");
    } else {
        println!("  ⬚  WhatsApp: disabled");
    }

    if config.channels.trello.enabled {
        let has_token = config
            .channels
            .trello
            .token
            .as_ref()
            .is_some_and(|t| !t.is_empty());
        let has_app_token = config
            .channels
            .trello
            .app_token
            .as_ref()
            .is_some_and(|t| !t.is_empty());
        if has_token && has_app_token {
            println!("  ✅ Trello: enabled, token + app token set");
        } else {
            println!("  ❌ Trello: enabled but missing credentials");
        }
    } else {
        println!("  ⬚  Trello: disabled");
    }
}

/// Memory operations
pub(crate) async fn cmd_memory(operation: MemoryCommands) -> Result<()> {
    let brain_path = BrainLoader::resolve_path();

    match operation {
        MemoryCommands::List => {
            println!("🦀 Memory Files\n");

            let brain_files = [
                "MEMORY.md",
                "USER.md",
                "AGENTS.md",
                "TOOLS.md",
                "SOUL.md",
                "persona.md",
                "system.md",
            ];

            println!("  Brain files:");
            for name in &brain_files {
                let path = brain_path.join(name);
                if path.exists() {
                    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    println!("    ✅ {name:<16} ({size} bytes)");
                }
            }

            let memory_dir = brain_path.join("memory");
            if memory_dir.exists() {
                let mut count = 0;
                let mut total_size = 0u64;
                if let Ok(entries) = std::fs::read_dir(&memory_dir) {
                    for entry in entries.flatten() {
                        if entry
                            .path()
                            .extension()
                            .is_some_and(|e| e == "md" || e == "txt")
                        {
                            count += 1;
                            total_size += entry.metadata().map(|m| m.len()).unwrap_or(0);
                        }
                    }
                }
                if count > 0 {
                    println!(
                        "\n  Memory directory: {count} file(s), {:.1} KB",
                        total_size as f64 / 1024.0
                    );
                    println!("    {}", memory_dir.display());
                }
            }

            println!();
            Ok(())
        }
        MemoryCommands::Get { name } => {
            let name = if name.ends_with(".md") {
                name
            } else {
                format!("{name}.md")
            };

            let path = brain_path.join(&name);
            let path = if path.exists() {
                path
            } else {
                let alt = brain_path.join("memory").join(&name);
                if alt.exists() {
                    alt
                } else {
                    anyhow::bail!("Memory file not found: {name}");
                }
            };

            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            println!("{content}");
            Ok(())
        }
        MemoryCommands::Stats => {
            println!("🦀 Memory Statistics\n");

            let brain_files: Vec<_> = ["MEMORY.md", "USER.md", "AGENTS.md", "TOOLS.md", "SOUL.md"]
                .iter()
                .filter(|f| brain_path.join(f).exists())
                .collect();

            let brain_size: u64 = brain_files
                .iter()
                .map(|f| {
                    std::fs::metadata(brain_path.join(f))
                        .map(|m| m.len())
                        .unwrap_or(0)
                })
                .sum();
            println!(
                "  Brain files:    {} ({:.1} KB)",
                brain_files.len(),
                brain_size as f64 / 1024.0
            );

            let memory_dir = brain_path.join("memory");
            if memory_dir.exists() {
                let (count, size) = std::fs::read_dir(&memory_dir)
                    .map(|rd| {
                        rd.flatten().fold((0u32, 0u64), |(c, s), e| {
                            (c + 1, s + e.metadata().map(|m| m.len()).unwrap_or(0))
                        })
                    })
                    .unwrap_or((0, 0));
                println!("  Memory entries: {count} ({:.1} KB)", size as f64 / 1024.0);
            }

            println!("  Brain path:     {}", brain_path.display());
            println!();
            Ok(())
        }
    }
}

/// Session management
pub(crate) async fn cmd_session(
    config: &crate::config::Config,
    operation: SessionCommands,
) -> Result<()> {
    use crate::{
        db::Database,
        services::{ServiceContext, SessionService},
    };

    let db = Database::connect(&config.database.path).await?;
    db.run_migrations().await?;
    let service_context = ServiceContext::new(db.pool().clone());
    let session_svc = SessionService::new(service_context);

    match operation {
        SessionCommands::List { all } => {
            use crate::db::repository::SessionListOptions;

            println!("🦀 Sessions\n");
            let options = SessionListOptions {
                include_archived: all,
                ..Default::default()
            };
            let sessions = session_svc.list_sessions(options).await?;

            if sessions.is_empty() {
                println!("  No sessions found");
            } else {
                for s in &sessions {
                    let title = s.title.as_deref().unwrap_or("untitled");
                    let provider = s.provider_name.as_deref().unwrap_or("-");
                    let model = s.model.as_deref().unwrap_or("-");
                    let tokens = s.token_count;
                    let archived = if s.archived_at.is_some() {
                        " [archived]"
                    } else {
                        ""
                    };
                    println!(
                        "  {} {:<30} {}/{} ({}tok){archived}",
                        &s.id.to_string()[..8],
                        title,
                        provider,
                        model,
                        tokens,
                    );
                }
                println!("\n  {} session(s)", sessions.len());
            }
            println!();
            Ok(())
        }
        SessionCommands::Get { id } => {
            let uuid = uuid::Uuid::parse_str(&id).context("Invalid session ID")?;
            match session_svc.get_session(uuid).await? {
                Some(s) => {
                    println!("🦀 Session {}\n", s.id);
                    println!("  Title:    {}", s.title.as_deref().unwrap_or("untitled"));
                    println!("  Provider: {}", s.provider_name.as_deref().unwrap_or("-"));
                    println!("  Model:    {}", s.model.as_deref().unwrap_or("-"));
                    println!("  Tokens:   {}", s.token_count);
                    println!("  Cost:     ${:.6}", s.total_cost);
                    println!("  Archived: {}", s.archived_at.is_some());
                    println!("  Created:  {}", s.created_at);
                    println!("  Updated:  {}", s.updated_at);
                    println!();
                }
                None => println!("Session not found: {id}"),
            }
            Ok(())
        }
    }
}

/// Resolve profile-specific service identifiers.
/// Default profile → `com.opencrabs.daemon` / `opencrabs`
/// Named profile → `com.opencrabs.daemon.hermes` / `opencrabs-hermes`
fn service_identifiers() -> (String, String, String) {
    let profile = crate::config::profile::active_profile();
    let suffix = match profile {
        Some(name) if name != "default" => format!(".{name}"),
        _ => String::new(),
    };
    let systemd_suffix = match profile {
        Some(name) if name != "default" => format!("-{name}"),
        _ => String::new(),
    };
    let plist_name = format!("com.opencrabs.daemon{suffix}");
    let systemd_name = format!("opencrabs{systemd_suffix}");
    let log_suffix = if suffix.is_empty() {
        String::new()
    } else {
        suffix.clone()
    };
    (plist_name, systemd_name, log_suffix)
}

/// Build the daemon arguments, including `-p <profile>` when a named profile is active.
fn daemon_args() -> Vec<String> {
    let mut args = Vec::new();
    if let Some(name) = crate::config::profile::active_profile()
        && name != "default"
    {
        args.push("-p".to_string());
        args.push(name.to_string());
    }
    args.push("daemon".to_string());
    args
}

/// Build a systemd unit file. A root-installed *system* unit runs the daemon
/// as the invoking user (`run_as`, taken from `SUDO_USER`) so it reads that
/// user's `~/.opencrabs` rather than root's, and targets `multi-user.target`
/// so it starts on boot. A *user* unit omits `User=`/`HOME=` and targets
/// `default.target`. Pure (no OS calls) so it is unit-testable on any platform.
// Only the Linux install path calls this; off-Linux it lives solely for tests.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn build_systemd_unit(
    profile_label: &str,
    exec_args: &str,
    run_as: Option<&str>,
    system: bool,
) -> String {
    let identity = match run_as {
        Some(u) => format!("User={u}\nGroup={u}\nEnvironment=HOME=/home/{u}\n"),
        None => String::new(),
    };
    let wanted_by = if system {
        "multi-user.target"
    } else {
        "default.target"
    };
    format!(
        "[Unit]\n\
         Description=OpenCrabs Daemon [{profile_label}]\n\
         After=network.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         {identity}ExecStart={exec_args}\n\
         Restart=always\n\
         RestartSec=5\n\
         \n\
         [Install]\n\
         WantedBy={wanted_by}\n"
    )
}

/// Linux: true when running as root (euid 0). We then install a *system* unit
/// (`/etc/systemd/system`, plain `systemctl`) instead of a user unit, because a
/// user unit needs a running per-user systemd + D-Bus session that a headless
/// VPS SSH session lacks ("Failed to connect to bus").
#[cfg(target_os = "linux")]
pub(crate) fn running_as_root() -> bool {
    // SAFETY: geteuid() always succeeds and has no preconditions.
    unsafe { libc::geteuid() == 0 }
}

/// Core probe logic with explicit paths for testability.
/// Returns `true` for system scope, `false` for user scope.
#[cfg(target_os = "linux")]
pub(crate) fn unit_scope_probe(
    systemd_name: &str,
    system_dir: &std::path::Path,
    user_dir: Option<&std::path::Path>,
) -> bool {
    if system_dir.join(format!("{systemd_name}.service")).exists() {
        return true;
    }
    if let Some(dir) = user_dir
        && dir.join(format!("{systemd_name}.service")).exists()
    {
        return false;
    }
    // Neither exists — fall back to the old heuristic (covers fresh install).
    running_as_root()
}

/// Probe which systemd scope a unit was installed in by checking the unit file
/// paths. Falls back to `running_as_root()` when neither file exists (e.g. the
/// unit has not been installed yet — `install` still decides scope on its own).
#[cfg(target_os = "linux")]
fn unit_scope(systemd_name: &str) -> bool {
    let system_dir = std::path::PathBuf::from("/etc/systemd/system");
    let user_dir = invoking_user_home().map(|h| h.join(".config/systemd/user"));
    unit_scope_probe(systemd_name, &system_dir, user_dir.as_deref())
}

/// Home of the user who actually owns the units. Under `sudo`,
/// `dirs::home_dir()` resolves to root's home (`/root`), not the invoking
/// user's — so a user-scope unit in `~/.config/systemd/user` would be missed
/// and we'd wrongly try system scope. Prefer `SUDO_USER`'s home, matching the
/// `/home/<user>` convention `install` already writes into the unit's
/// `Environment=HOME=`.
#[cfg(target_os = "linux")]
fn invoking_user_home() -> Option<std::path::PathBuf> {
    user_home_from(std::env::var("SUDO_USER").ok().as_deref(), dirs::home_dir())
}

/// Pure resolution for [`invoking_user_home`]: prefer `SUDO_USER`'s standard
/// home (`/home/<user>`), otherwise the `fallback`. Split out so it is testable
/// without touching the environment or filesystem.
#[cfg(target_os = "linux")]
pub(crate) fn user_home_from(
    sudo_user: Option<&str>,
    fallback: Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    match sudo_user.filter(|u| !u.is_empty() && *u != "root") {
        Some(user) => Some(std::path::PathBuf::from(format!("/home/{user}"))),
        None => fallback,
    }
}

/// Run `systemctl` (system scope, or `--user`) and FAIL on a non-zero exit.
/// `.status()` alone only catches a spawn failure, so "Failed to connect to
/// bus" (exit 1) used to slip through and the caller printed a misleading ✅.
#[cfg(target_os = "linux")]
fn run_systemctl(system: bool, args: &[&str]) -> Result<()> {
    let mut cmd = std::process::Command::new("systemctl");
    if !system {
        cmd.arg("--user");
    }
    let out = cmd
        .args(args)
        .output()
        .context("failed to spawn systemctl")?;
    if out.status.success() {
        return Ok(());
    }
    let scope = if system {
        "systemctl"
    } else {
        "systemctl --user"
    };
    anyhow::bail!(
        "{scope} {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr).trim()
    );
}

/// Run `launchctl <flags> <plist>` and FAIL on a non-zero exit, except for its
/// benign no-ops (loading an already-loaded agent, unloading one that isn't
/// loaded). Like the Linux path, `.status()` alone hid real failures behind a
/// green ✅.
#[cfg(target_os = "macos")]
fn run_launchctl(flags: &[&str], plist: &std::path::Path) -> Result<()> {
    let out = std::process::Command::new("launchctl")
        .args(flags)
        .arg(plist)
        .output()
        .context("failed to spawn launchctl")?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let lc = stderr.to_lowercase();
    if lc.contains("already loaded") || lc.contains("not find") || lc.contains("no such") {
        return Ok(());
    }
    anyhow::bail!(
        "launchctl {} {} failed: {}",
        flags.join(" "),
        plist.display(),
        stderr.trim()
    );
}

/// Guidance shown when a `--user` systemctl op fails for lack of a user bus.
#[cfg(target_os = "linux")]
fn user_service_hint() -> String {
    "A user service needs a running per-user systemd instance, which a headless \
     SSH session usually lacks. Either enable lingering so it runs without an \
     active login:\n    sudo loginctl enable-linger $(whoami)\nthen retry, or \
     install a system-wide service instead:\n    sudo opencrabs service install"
        .to_string()
}

/// OS service management
#[allow(unused_variables)]
pub(crate) async fn cmd_service(operation: ServiceCommands) -> Result<()> {
    let binary = std::env::current_exe().context("Could not determine binary path")?;
    let binary_str = binary.display().to_string();
    let (plist_name, systemd_name, log_suffix) = service_identifiers();
    let args = daemon_args();
    let profile_label = crate::config::profile::active_profile().unwrap_or("default");

    match operation {
        ServiceCommands::Install => {
            #[cfg(target_os = "macos")]
            {
                let plist_path = dirs::home_dir()
                    .context("No home dir")?
                    .join("Library/LaunchAgents")
                    .join(format!("{plist_name}.plist"));

                let args_xml: String =
                    std::iter::once(format!("        <string>{binary_str}</string>"))
                        .chain(args.iter().map(|a| format!("        <string>{a}</string>")))
                        .collect::<Vec<_>>()
                        .join("\n");

                let plist = format!(
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{plist_name}</string>
    <key>ProgramArguments</key>
    <array>
{args_xml}
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/tmp/opencrabs-daemon{log_suffix}.out.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/opencrabs-daemon{log_suffix}.err.log</string>
</dict>
</plist>"#
                );

                if let Some(parent) = plist_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&plist_path, plist)?;
                println!(
                    "✅ Installed LaunchAgent [{profile_label}]: {}",
                    plist_path.display()
                );
                println!("   Run: opencrabs service start");
            }

            #[cfg(target_os = "linux")]
            {
                let system = running_as_root();
                let unit_path = if system {
                    std::path::PathBuf::from(format!("/etc/systemd/system/{systemd_name}.service"))
                } else {
                    dirs::home_dir()
                        .context("No home dir")?
                        .join(format!(".config/systemd/user/{systemd_name}.service"))
                };

                let exec_args = std::iter::once(binary_str.clone())
                    .chain(args.iter().cloned())
                    .collect::<Vec<_>>()
                    .join(" ");

                // System unit installed via sudo: run the daemon as the
                // invoking user so it reads that user's ~/.opencrabs, not root's.
                let run_as = if system {
                    std::env::var("SUDO_USER")
                        .ok()
                        .filter(|u| !u.is_empty() && u != "root")
                } else {
                    None
                };
                let unit = build_systemd_unit(profile_label, &exec_args, run_as.as_deref(), system);

                if let Some(parent) = unit_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&unit_path, unit)
                    .with_context(|| format!("writing unit {}", unit_path.display()))?;

                if let Err(e) = run_systemctl(system, &["daemon-reload"]) {
                    eprintln!("❌ {e}");
                    if !system {
                        eprintln!("\n{}", user_service_hint());
                    }
                    return Err(e);
                }
                println!(
                    "✅ Installed systemd {} unit [{profile_label}]: {}",
                    if system { "system" } else { "user" },
                    unit_path.display()
                );
                println!(
                    "   Run: {}opencrabs service start",
                    if system { "sudo " } else { "" }
                );
            }

            #[cfg(not(any(target_os = "macos", target_os = "linux")))]
            return Err(anyhow::anyhow!(
                "Service install not supported on this platform"
            ));

            #[cfg(any(target_os = "macos", target_os = "linux"))]
            Ok(())
        }
        ServiceCommands::Start => {
            #[cfg(target_os = "macos")]
            {
                let plist = dirs::home_dir()
                    .context("No home dir")?
                    .join(format!("Library/LaunchAgents/{plist_name}.plist"));
                if let Err(e) = run_launchctl(&["load", "-w"], &plist) {
                    eprintln!("❌ {e}");
                    return Err(e);
                }
                println!("✅ Started OpenCrabs daemon [{profile_label}]");
            }

            #[cfg(target_os = "linux")]
            {
                let system = unit_scope(&systemd_name);
                // `enable --now` so a system unit also survives reboot; a user
                // unit just starts (boot-persistence there needs lingering).
                let res = if system {
                    run_systemctl(true, &["enable", "--now", systemd_name.as_str()])
                } else {
                    run_systemctl(false, &["start", systemd_name.as_str()])
                };
                if let Err(e) = res {
                    eprintln!("❌ {e}");
                    if !system {
                        eprintln!("\n{}", user_service_hint());
                    }
                    return Err(e);
                }
                println!("✅ Started OpenCrabs daemon [{profile_label}]");
            }

            Ok(())
        }
        ServiceCommands::Stop => {
            #[cfg(target_os = "macos")]
            {
                let plist = dirs::home_dir()
                    .context("No home dir")?
                    .join(format!("Library/LaunchAgents/{plist_name}.plist"));
                if let Err(e) = run_launchctl(&["unload"], &plist) {
                    eprintln!("❌ {e}");
                    return Err(e);
                }
                println!("✅ Stopped OpenCrabs daemon [{profile_label}]");
            }

            #[cfg(target_os = "linux")]
            {
                let system = unit_scope(&systemd_name);
                if let Err(e) = run_systemctl(system, &["stop", systemd_name.as_str()]) {
                    eprintln!("❌ {e}");
                    if !system {
                        eprintln!("\n{}", user_service_hint());
                    }
                    return Err(e);
                }
                println!("✅ Stopped OpenCrabs daemon [{profile_label}]");
            }

            Ok(())
        }
        ServiceCommands::Restart => {
            #[cfg(target_os = "macos")]
            {
                let plist = dirs::home_dir()
                    .context("No home dir")?
                    .join(format!("Library/LaunchAgents/{plist_name}.plist"));
                let _ = run_launchctl(&["unload"], &plist);
                if let Err(e) = run_launchctl(&["load", "-w"], &plist) {
                    eprintln!("❌ {e}");
                    return Err(e);
                }
                println!("✅ Restarted OpenCrabs daemon [{profile_label}]");
            }
            #[cfg(target_os = "linux")]
            {
                let system = unit_scope(&systemd_name);
                if let Err(e) = run_systemctl(system, &["restart", systemd_name.as_str()]) {
                    eprintln!("❌ {e}");
                    if !system {
                        eprintln!("\n{}", user_service_hint());
                    }
                    return Err(e);
                }
                println!("✅ Restarted OpenCrabs daemon [{profile_label}]");
            }
            Ok(())
        }
        ServiceCommands::Status => {
            #[cfg(target_os = "macos")]
            {
                let output = std::process::Command::new("launchctl")
                    .args(["list", &plist_name])
                    .output()?;
                if output.status.success() {
                    println!("✅ OpenCrabs daemon [{profile_label}] is running");
                    println!("{}", String::from_utf8_lossy(&output.stdout));
                } else {
                    println!("⬚  OpenCrabs daemon [{profile_label}] is not running");
                }
            }

            #[cfg(target_os = "linux")]
            {
                let system = unit_scope(&systemd_name);
                let mut cmd = std::process::Command::new("systemctl");
                if !system {
                    cmd.arg("--user");
                }
                let output = cmd.args(["status", systemd_name.as_str()]).output()?;
                // `systemctl status` exits non-zero when the unit is inactive;
                // that is informational, not an error, so just show the output.
                print!("{}", String::from_utf8_lossy(&output.stdout));
                if output.stdout.is_empty() {
                    eprint!("{}", String::from_utf8_lossy(&output.stderr));
                }
            }

            Ok(())
        }
        ServiceCommands::Uninstall => {
            #[cfg(target_os = "macos")]
            {
                let plist = dirs::home_dir()
                    .context("No home dir")?
                    .join(format!("Library/LaunchAgents/{plist_name}.plist"));
                let _ = std::process::Command::new("launchctl")
                    .args(["unload"])
                    .arg(&plist)
                    .status();
                if plist.exists() {
                    std::fs::remove_file(&plist)?;
                    println!(
                        "✅ Removed LaunchAgent [{profile_label}]: {}",
                        plist.display()
                    );
                } else {
                    println!("⬚  LaunchAgent [{profile_label}] not found");
                }
            }
            #[cfg(target_os = "linux")]
            {
                let system = unit_scope(&systemd_name);
                // Best-effort stop/disable: failing here (already stopped, no
                // bus) must not block removing the unit file.
                let _ = run_systemctl(system, &["stop", systemd_name.as_str()]);
                let _ = run_systemctl(system, &["disable", systemd_name.as_str()]);
                let unit_path = if system {
                    std::path::PathBuf::from(format!("/etc/systemd/system/{systemd_name}.service"))
                } else {
                    dirs::home_dir()
                        .context("No home dir")?
                        .join(format!(".config/systemd/user/{systemd_name}.service"))
                };
                if unit_path.exists() {
                    std::fs::remove_file(&unit_path)?;
                    let _ = run_systemctl(system, &["daemon-reload"]);
                    println!(
                        "✅ Removed systemd unit [{profile_label}]: {}",
                        unit_path.display()
                    );
                } else {
                    println!("⬚  Systemd unit [{profile_label}] not found");
                }
            }

            Ok(())
        }
    }
}

/// Profile management
pub(crate) async fn cmd_profile(operation: ProfileCommands) -> Result<()> {
    use crate::config::profile;

    match operation {
        ProfileCommands::Create { name, description } => {
            let path = profile::create_profile(&name, description.as_deref())?;
            println!("✅ Created profile '{name}'");
            println!("   Path: {}", path.display());
            println!("\n   Usage: opencrabs -p {name}");
            Ok(())
        }
        ProfileCommands::List => {
            let profiles = profile::list_profiles()?;
            let active = profile::active_profile().unwrap_or("default");

            println!("Profiles:\n");
            for p in &profiles {
                let marker = if p.name == active { " ←" } else { "" };
                let desc = p
                    .description
                    .as_deref()
                    .map(|d| format!(" — {d}"))
                    .unwrap_or_default();
                println!("  {}{}{}", p.name, desc, marker);
            }
            println!("\n  {} profile(s) total", profiles.len());
            Ok(())
        }
        ProfileCommands::Delete { name, force } => {
            if !force {
                println!("⚠️  This will permanently delete profile '{name}' and ALL its data.");
                println!("   Re-run with --force to confirm.");
                return Ok(());
            }
            profile::delete_profile(&name)?;
            println!("✅ Deleted profile '{name}'");
            Ok(())
        }
        ProfileCommands::Export { name, output } => {
            let output_path = output
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from(format!("{name}.tar.gz")));
            profile::export_profile(&name, &output_path)?;
            println!("✅ Exported profile '{name}' to {}", output_path.display());
            Ok(())
        }
        ProfileCommands::Import { path } => {
            let name = profile::import_profile(std::path::Path::new(&path))?;
            println!("✅ Imported profile '{name}'");
            println!("\n   Usage: opencrabs -p {name}");
            Ok(())
        }
        ProfileCommands::Migrate { from, to, force } => {
            let migrated = profile::migrate_profile(&from, &to, force)?;
            if migrated.is_empty() {
                println!("⚠️  No files migrated from '{from}' to '{to}'.");
                println!("   All files already exist in '{to}'. Use --force to overwrite.");
            } else {
                println!(
                    "✅ Migrated {} files from '{from}' to '{to}':\n",
                    migrated.len()
                );
                for file in &migrated {
                    println!("   {file}");
                }
                println!("\n   Switch to the new profile: opencrabs -p {to}");
                println!("   Then customize identity, brain files, keys, etc.");
            }
            Ok(())
        }
    }
}

/// Check for and install the latest OpenCrabs release
pub(crate) async fn cmd_evolve(check_only: bool) -> Result<()> {
    let current_version = crate::VERSION;
    println!("🔄 OpenCrabs v{current_version} — checking for updates...\n");

    if check_only {
        match crate::brain::tools::evolve::check_for_update().await {
            Some(latest) => {
                println!("✅ Update available: v{current_version} → v{latest}");
                println!("   Run `opencrabs evolve` to install.");
            }
            None => {
                println!("✅ Already on the latest version (v{current_version}).");
            }
        }
        return Ok(());
    }

    // Full evolve — instantiate the tool and execute
    use crate::brain::tools::evolve::EvolveTool;
    use crate::brain::tools::{Tool, ToolExecutionContext};
    use serde_json::json;
    use std::collections::HashMap;

    let tool = EvolveTool::new(None);
    let input = json!({"check_only": false});
    let context = ToolExecutionContext {
        session_id: uuid::Uuid::new_v4(),
        working_directory: std::env::current_dir().unwrap_or_default(),
        env_vars: HashMap::new(),
        auto_approve: true, // CLI user explicitly invoked evolve
        timeout_secs: 300,
        sudo_callback: None,
        ssh_callback: None,
        shared_working_directory: None,
        service_context: None,
        question_callback: None,
    };

    let result = tool.execute(input, &context).await?;
    println!("{}", result.output);

    Ok(())
}
