//! CLI Module
//!
//! Command-line interface for OpenCrab using Clap v4.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::sync::Arc;

/// System prompt that encourages proactive tool usage for codebase exploration
const SYSTEM_PROMPT: &str = r#"You are OpenCrab, an AI orchestration agent with powerful tools to help with software development tasks.

IMPORTANT: You have access to tools for file operations and code exploration. USE THEM PROACTIVELY!

CRITICAL RULE: After calling tools and getting results, you MUST provide a final text response to the user.
DO NOT keep calling tools in a loop. Call the necessary tools, get results, then respond with text.

When asked to analyze or explore a codebase:
1. Use 'ls' tool with recursive=true to list all directories and files
2. Use 'glob' tool with patterns like "**/*.rs", "**/*.toml", "**/*.md" to find files
3. Use 'grep' tool to search for patterns, functions, or keywords in code
4. Use 'read_file' tool to read specific files you've identified
5. Use 'bash' tool for git operations like: git log, git diff, git branch

When asked to make changes:
1. Use 'read_file' first to understand the current code
2. Use 'edit_file' to modify existing files
3. Use 'write_file' to create new files
4. Use 'bash' to run tests or build commands

Available tools and when to use them:
- ls: List directory contents (use recursive=true for deep exploration)
- glob: Find files matching patterns (e.g., "**/*.rs" for all Rust files)
- grep: Search for text/patterns in files (use for finding functions, TODOs, etc.)
- read_file: Read file contents
- edit_file: Modify existing files
- write_file: Create new files
- bash: Run shell commands (git, cargo, npm, etc.)
- execute_code: Test code snippets
- web_search: Search the internet for documentation
- http_request: Call external APIs
- task_manager: Track multi-step work
- session_context: Remember important facts
- plan: Create structured plans for complex tasks (use when user requests require multiple coordinated steps)

CRITICAL: PLAN TOOL USAGE
When a user says "create a plan", "make a plan", or describes a complex multi-step task, you MUST use the plan tool immediately.
DO NOT write a text description of a plan. DO NOT explain what should be done. CALL THE TOOL.

Mandatory steps for plan creation:
1. IMMEDIATELY call plan tool with operation='create' to create a new plan
2. Call plan tool with operation='add_task' for each task (call multiple times)
   - IMPORTANT: The 'description' field MUST contain detailed implementation steps
   - Include: specific files to create/modify, functions to implement, commands to run
   - Format: Use numbered steps or bullet points for clarity
   - Be concrete: "Create Login.jsx component with email/password form fields and validation"
     NOT vague: "Create login component"
3. Call plan tool with operation='finalize' to present the plan for user approval
4. **STOP CALLING TOOLS** - After 'finalize', DO NOT call any more plan operations!
5. INFORM the user that the plan is ready for review:
   "✅ Plan finalized! The plan is now displayed in Plan Mode for your review.

   To proceed:
   • Press Ctrl+A to approve and execute the plan
   • Press Ctrl+R to reject and revise the plan
   • Press Esc to cancel and return to chat

   When you approve, the plan will be automatically exported to PLAN.md and execution will begin."
6. WAIT for the user to approve the plan via Ctrl+A before execution begins
   - The TUI will automatically switch to Plan Mode and display the plan
   - User controls the approval through keyboard shortcuts, not text responses
   - Your job is DONE after calling finalize and informing the user

IMPORTANT: Do NOT call plan tool with operation='export_markdown' after finalize.
The markdown export happens automatically when the user presses Ctrl+A to approve the plan.

Example: If user says "create a plan to implement a login page"
- FIRST TOOL CALL: plan(operation="create", title="Implement Login Page", description="Build a React login page with email/password authentication", context="React app needs user authentication. Backend API endpoint /auth/login exists.")
- NEXT TOOL CALL: plan(operation="add_task", title="Create Login Component", description="1. Create src/components/Login.jsx file\n2. Add email input field with type='email' validation\n3. Add password input field with type='password'\n4. Add submit button that calls handleSubmit()\n5. Import useState for form state management\n6. Add basic CSS styling for form layout", task_type="create", complexity=2)
- NEXT TOOL CALL: plan(operation="add_task", title="Implement Authentication Logic", description="1. Create handleSubmit() function in Login.jsx\n2. Validate email format using regex\n3. Make POST request to /auth/login endpoint\n4. Include email/password in request body\n5. Handle success response - store JWT token in localStorage\n6. Handle error response - display error message to user\n7. Redirect to dashboard on successful login", task_type="edit", complexity=3, dependencies=[1])
- TOOL CALL: plan(operation="finalize")
- THEN SAY: "✅ Plan finalized! The plan is now displayed in Plan Mode. Press Ctrl+A to approve and execute, Ctrl+R to reject, or Esc to cancel. The plan will be exported to PLAN.md when you approve it."

TASK DESCRIPTION QUALITY REQUIREMENTS:
- Each task description MUST be detailed enough to execute without further clarification
- Include specific file paths, function names, and concrete implementation steps
- Mention required libraries, APIs, or dependencies
- Specify error handling and edge cases
- Add configuration or setup requirements

NEVER generate text plans. ALWAYS use the plan tool for planning requests.

ALWAYS explore first before answering questions about a codebase. Don't guess - use the tools!"#;

/// OpenCrab - High-Performance Terminal AI Orchestration Agent
#[derive(Parser, Debug)]
#[command(name = "opencrab")]
#[command(version, about, long_about = None)]
pub struct Cli {
    /// Enable debug mode (creates log files in .opencrab/logs/)
    #[arg(short, long, global = true)]
    pub debug: bool,

    /// Configuration file path
    #[arg(short, long, global = true)]
    pub config: Option<String>,

    /// Subcommand to execute
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Start interactive TUI mode (default)
    Chat {
        /// Session ID to resume
        #[arg(short, long)]
        session: Option<String>,
    },

    /// Run a single command non-interactively
    Run {
        /// The prompt to execute
        prompt: String,

        /// Auto-approve all tool executions (dangerous!)
        #[arg(long, alias = "yolo")]
        auto_approve: bool,

        /// Output format
        #[arg(short, long, default_value = "text")]
        format: OutputFormat,
    },

    /// Initialize configuration
    Init {
        /// Force overwrite existing configuration
        #[arg(short, long)]
        force: bool,
    },

    /// Show configuration
    Config {
        /// Show full configuration including secrets
        #[arg(short, long)]
        show_secrets: bool,
    },

    /// Database operations
    Db {
        #[command(subcommand)]
        operation: DbCommands,
    },

    /// Log management operations
    Logs {
        #[command(subcommand)]
        operation: LogCommands,
    },

    /// Manage API keys in OS keyring (secure storage)
    Keyring {
        #[command(subcommand)]
        operation: KeyringCommands,
    },
}

#[derive(Subcommand, Debug)]
pub enum LogCommands {
    /// Show log file location and status
    Status,
    /// View recent log entries (requires debug mode)
    View {
        /// Number of lines to show (default: 50)
        #[arg(short, long, default_value = "50")]
        lines: usize,
    },
    /// Clean up old log files
    Clean {
        /// Maximum age in days (default: 7)
        #[arg(short = 'a', long, default_value = "7")]
        days: u64,
    },
    /// Open log directory in file manager
    Open,
}

#[derive(Subcommand, Debug)]
pub enum DbCommands {
    /// Initialize database
    Init,
    /// Show database statistics
    Stats,
    /// Clear all sessions and messages from database
    Clear {
        /// Skip confirmation prompt (use with caution)
        #[arg(short, long)]
        force: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum KeyringCommands {
    /// Store an API key in OS keyring
    Set {
        /// Provider name (anthropic, openai, gemini, azure)
        provider: String,
        /// API key to store
        api_key: String,
    },
    /// Retrieve an API key from OS keyring
    Get {
        /// Provider name
        provider: String,
    },
    /// Delete an API key from OS keyring
    Delete {
        /// Provider name
        provider: String,
    },
    /// List all stored providers
    List,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
    Markdown,
}

/// Main CLI entry point
pub async fn run() -> Result<()> {
    let cli = Cli::parse();

    // Set up logging level based on debug flag
    if cli.debug {
        tracing::info!("Debug mode enabled");
    }

    // Load configuration
    let config = load_config(cli.config.as_deref()).await?;

    match cli.command {
        None | Some(Commands::Chat { session: _ }) => {
            // Default: Interactive TUI mode
            let session = match &cli.command {
                Some(Commands::Chat { session }) => session.clone(),
                _ => None,
            };
            cmd_chat(&config, session).await
        }
        Some(Commands::Init { force }) => cmd_init(&config, force).await,
        Some(Commands::Config { show_secrets }) => cmd_config(&config, show_secrets).await,
        Some(Commands::Db { operation }) => cmd_db(&config, operation).await,
        Some(Commands::Logs { operation }) => cmd_logs(operation).await,
        Some(Commands::Keyring { operation }) => cmd_keyring(operation).await,
        Some(Commands::Run {
            prompt,
            auto_approve,
            format,
        }) => cmd_run(&config, prompt, auto_approve, format).await,
    }
}

/// Load configuration from file or defaults
async fn load_config(config_path: Option<&str>) -> Result<crate::config::Config> {
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
async fn cmd_init(_config: &crate::config::Config, force: bool) -> Result<()> {
    use crate::config::Config;

    println!("🦀 OpenCrab Configuration Initialization\n");

    let config_path = dirs::config_dir()
        .context("Could not determine config directory")?
        .join("opencrab")
        .join("config.toml");

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
    println!("   3. Run 'opencrab' or 'opencrab chat' to start");

    Ok(())
}

/// Show configuration
async fn cmd_config(config: &crate::config::Config, show_secrets: bool) -> Result<()> {
    println!("🦀 OpenCrab Configuration\n");

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
async fn cmd_db(config: &crate::config::Config, operation: DbCommands) -> Result<()> {
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

            // Get counts using raw SQL for simplicity
            let session_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
                .fetch_one(db.pool())
                .await?;

            let message_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
                .fetch_one(db.pool())
                .await?;

            let file_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM files")
                .fetch_one(db.pool())
                .await?;

            println!("Sessions: {}", session_count);
            println!("Messages: {}", message_count);
            println!("Files: {}", file_count);

            Ok(())
        }
        DbCommands::Clear { force } => {
            let db = Database::connect(&config.database.path).await?;

            // Get counts before clearing
            let session_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
                .fetch_one(db.pool())
                .await?;

            let message_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
                .fetch_one(db.pool())
                .await?;

            let file_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM files")
                .fetch_one(db.pool())
                .await?;

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
            sqlx::query("DELETE FROM messages")
                .execute(db.pool())
                .await?;

            sqlx::query("DELETE FROM files").execute(db.pool()).await?;

            sqlx::query("DELETE FROM sessions")
                .execute(db.pool())
                .await?;

            println!(
                "✅ Successfully cleared {} sessions, {} messages, and {} files",
                session_count, message_count, file_count
            );

            Ok(())
        }
    }
}

/// Start interactive chat session
async fn cmd_chat(config: &crate::config::Config, _session_id: Option<String>) -> Result<()> {
    use crate::{
        db::Database,
        llm::{
            agent::AgentService,
            tools::{
                bash::BashTool, code_exec::CodeExecTool, context::ContextTool,
                doc_parser::DocParserTool, edit::EditTool, glob::GlobTool, grep::GrepTool,
                http::HttpClientTool, ls::LsTool, notebook::NotebookEditTool, plan_tool::PlanTool,
                read::ReadTool, registry::ToolRegistry, task::TaskTool, web_search::WebSearchTool,
                write::WriteTool,
            },
        },
        services::ServiceContext,
        tui,
    };

    println!("🦀 Starting OpenCrab AI Orchestration Agent...\n");

    // Initialize database
    tracing::info!("Connecting to database: {}", config.database.path.display());
    let db = Database::connect(&config.database.path)
        .await
        .context("Failed to connect to database")?;

    // Run migrations
    db.run_migrations()
        .await
        .context("Failed to run database migrations")?;

    // Select provider based on configuration using factory
    let provider = crate::llm::provider::create_provider(config)?;

    // OLD CODE - REPLACED BY FACTORY PATTERN
    /*
    let provider: Arc<dyn Provider> = if let Some(qwen_config) = &config.providers.qwen {
        // Qwen provider is configured
        if let Some(base_url) = &qwen_config.base_url {
            // Local Qwen (vLLM, LM Studio, etc.)
            tracing::info!("Using local Qwen at: {}", base_url);
            println!("🏠 Using local Qwen at: {}\n", base_url);
            let mut provider = QwenProvider::local(base_url.clone());

            // Set tool parser
            if let Some(parser) = &qwen_config.tool_parser {
                let tool_parser = match parser.as_str() {
                    "openai" => ToolCallParser::OpenAI,
                    "native" | "qwen" => ToolCallParser::NativeQwen,
                    _ => ToolCallParser::Hermes, // Default to Hermes for Qwen
                };
                provider = provider.with_tool_parser(tool_parser);
                tracing::info!("Using tool parser: {:?}", tool_parser);
                if tool_parser == ToolCallParser::NativeQwen {
                    println!("🔧 Using native Qwen function calling (✿FUNCTION✿ markers)\n");
                }
            }

            // Set thinking mode
            if qwen_config.enable_thinking {
                provider = provider.with_thinking(true);
                tracing::info!("🧠 Qwen3 thinking mode enabled");
                println!("🧠 Thinking mode: enabled\n");
                if let Some(budget) = qwen_config.thinking_budget {
                    provider = provider.with_thinking_budget(budget);
                    tracing::info!("Thinking budget: {} tokens", budget);
                }
            }

            if let Some(model) = &qwen_config.default_model {
                tracing::info!("Using custom default model: {}", model);
                println!("📦 Model: {}\n", model);
                provider = provider.with_default_model(model.clone());
            }
            Arc::new(provider)
        } else if let Some(api_key) = &qwen_config.api_key {
            // DashScope cloud API
            let region = qwen_config.region.as_deref().unwrap_or("intl");
            let provider_base = match region {
                "cn" => {
                    tracing::info!("Using DashScope China (Beijing)");
                    println!("☁️  Using DashScope China (Beijing)\n");
                    QwenProvider::dashscope_cn(api_key.clone())
                }
                _ => {
                    tracing::info!("Using DashScope International (Singapore)");
                    println!("☁️  Using DashScope International (Singapore)\n");
                    QwenProvider::dashscope_intl(api_key.clone())
                }
            };

            let mut provider = provider_base;

            // Set tool parser (default to OpenAI for cloud)
            if let Some(parser) = &qwen_config.tool_parser {
                let tool_parser = match parser.as_str() {
                    "hermes" => ToolCallParser::Hermes,
                    "native" | "qwen" => ToolCallParser::NativeQwen,
                    _ => ToolCallParser::OpenAI,
                };
                provider = provider.with_tool_parser(tool_parser);
                if tool_parser == ToolCallParser::NativeQwen {
                    println!("🔧 Using native Qwen function calling (✿FUNCTION✿ markers)\n");
                }
            }

            // Set thinking mode
            if qwen_config.enable_thinking {
                provider = provider.with_thinking(true);
                tracing::info!("🧠 Qwen3 thinking mode enabled");
                println!("🧠 Thinking mode: enabled\n");
                if let Some(budget) = qwen_config.thinking_budget {
                    provider = provider.with_thinking_budget(budget);
                }
            }

            if let Some(model) = &qwen_config.default_model {
                tracing::info!("Using custom default model: {}", model);
                println!("📦 Model: {}\n", model);
                provider = provider.with_default_model(model.clone());
            }
            Arc::new(provider)
        } else {
            // Qwen configured but no credentials - fall back to OpenAI/Anthropic
            tracing::debug!("Qwen configured but no credentials, falling back");
            create_fallback_provider(config)?
        }
    } else if let Some(openai_config) = &config.providers.openai {
        // OpenAI provider is configured
        if let Some(base_url) = &openai_config.base_url {
            // Local LLM (LM Studio, Ollama, etc.)
            tracing::info!("Using local LLM at: {}", base_url);
            println!("🏠 Using local LLM at: {}\n", base_url);
            let mut provider = OpenAIProvider::local(base_url.clone());
            if let Some(model) = &openai_config.default_model {
                tracing::info!("Using custom default model: {}", model);
                println!("📦 Model: {}\n", model);
                provider = provider.with_default_model(model.clone());
            }
            Arc::new(provider)
        } else if let Some(api_key) = &openai_config.api_key {
            // Official OpenAI API
            tracing::info!("Using OpenAI provider");
            println!("🤖 Using OpenAI provider\n");
            let mut provider = OpenAIProvider::new(api_key.clone());
            if let Some(model) = &openai_config.default_model {
                tracing::info!("Using custom default model: {}", model);
                println!("📦 Model: {}\n", model);
                provider = provider.with_default_model(model.clone());
            }
            Arc::new(provider)
        } else {
            // OpenAI configured but no credentials - fall back to Anthropic
            tracing::debug!("OpenAI configured but no credentials, falling back to Anthropic");
            let anthropic_config = config.providers.anthropic.as_ref().context(
                "No provider configured. Please set ANTHROPIC_API_KEY or OPENAI_API_KEY",
            )?;

            let api_key = anthropic_config
                .api_key
                .as_ref()
                .context("Anthropic API key not set")?
                .clone();

            tracing::info!("Using Anthropic provider");
            println!("🤖 Using Anthropic Claude\n");
            Arc::new(AnthropicProvider::new(api_key))
        }
    } else {
        // No OpenAI config, use Anthropic
        let anthropic_config = config
            .providers
            .anthropic
            .as_ref()
            .context("No provider configured.\n\nPlease set one of:\n  - ANTHROPIC_API_KEY for Claude\n  - OPENAI_API_KEY for OpenAI/GPT\n  - OPENAI_BASE_URL for local LLMs (LM Studio, Ollama)\n  - QWEN_BASE_URL for local Qwen (vLLM)\n  - DASHSCOPE_API_KEY for DashScope cloud\n\nExample for vLLM with Qwen:\n  export QWEN_BASE_URL=\"http://localhost:8000/v1/chat/completions\"")?;

        let api_key = anthropic_config
            .api_key
            .as_ref()
            .context("Anthropic API key not set")?
            .clone();

        tracing::info!("Using Anthropic provider");
        println!("🤖 Using Anthropic Claude\n");
        Arc::new(AnthropicProvider::new(api_key))
    };

    // Helper function for fallback provider - REMOVED, now in factory module
    */

    // Create tool registry
    tracing::debug!("Setting up tool registry");
    let mut tool_registry = ToolRegistry::new();
    // Phase 1: Essential file operations
    tool_registry.register(Arc::new(ReadTool));
    tool_registry.register(Arc::new(WriteTool));
    tool_registry.register(Arc::new(EditTool));
    tool_registry.register(Arc::new(BashTool));
    tool_registry.register(Arc::new(LsTool));
    tool_registry.register(Arc::new(GlobTool));
    tool_registry.register(Arc::new(GrepTool));
    // Phase 2: Advanced features
    tool_registry.register(Arc::new(WebSearchTool));
    tool_registry.register(Arc::new(CodeExecTool));
    tool_registry.register(Arc::new(NotebookEditTool));
    tool_registry.register(Arc::new(DocParserTool));
    // Phase 3: Workflow & integration
    tool_registry.register(Arc::new(TaskTool));
    tool_registry.register(Arc::new(ContextTool));
    tool_registry.register(Arc::new(HttpClientTool));
    tool_registry.register(Arc::new(PlanTool));

    // Create service context
    let service_context = ServiceContext::new(db.pool().clone());

    // Get working directory
    let working_directory = std::env::current_dir().unwrap_or_default();

    // Create agent service with system prompt and working directory
    let agent_service = Arc::new(
        AgentService::new(provider.clone(), service_context.clone())
            .with_system_prompt(SYSTEM_PROMPT.to_string())
            .with_max_tool_iterations(20)
            .with_working_directory(working_directory.clone()),
    );

    // Create TUI app first (so we can get the event sender)
    tracing::debug!("Creating TUI app");
    let mut app = tui::App::new(agent_service, service_context.clone());

    // Get event sender from app
    let event_sender = app.event_sender();

    // Create approval callback that sends requests to TUI
    let approval_callback: crate::llm::agent::ApprovalCallback = Arc::new(move |tool_info| {
        let sender = event_sender.clone();
        Box::pin(async move {
            use crate::tui::events::{ToolApprovalRequest, TuiEvent};
            use tokio::sync::mpsc;

            // Create response channel
            let (response_tx, mut response_rx) = mpsc::unbounded_channel();

            // Create approval request
            let request = ToolApprovalRequest {
                request_id: uuid::Uuid::new_v4(),
                tool_name: tool_info.tool_name,
                tool_description: tool_info.tool_description,
                tool_input: tool_info.tool_input,
                capabilities: tool_info.capabilities,
                response_tx,
                requested_at: std::time::Instant::now(),
            };

            // Send to TUI
            sender
                .send(TuiEvent::ToolApprovalRequested(request))
                .map_err(|e| {
                    crate::llm::agent::AgentError::Internal(format!(
                        "Failed to send approval request: {}",
                        e
                    ))
                })?;

            // Wait for response
            let response = response_rx.recv().await.ok_or_else(|| {
                crate::llm::agent::AgentError::Internal(
                    "Approval response channel closed".to_string(),
                )
            })?;

            Ok(response.approved)
        })
    });

    // Create agent service with approval callback
    tracing::debug!("Creating agent service with approval callback");
    let agent_service = Arc::new(
        AgentService::new(provider.clone(), service_context.clone())
            .with_system_prompt(SYSTEM_PROMPT.to_string())
            .with_tool_registry(Arc::new(tool_registry))
            .with_approval_callback(Some(approval_callback))
            .with_max_tool_iterations(20)
            .with_working_directory(working_directory),
    );

    // Update app with the configured agent service (preserve event channels!)
    app.set_agent_service(agent_service);

    // Run TUI
    tracing::debug!("Launching TUI");
    tui::run(app).await.context("TUI error")?;

    println!("\n👋 Goodbye!");

    Ok(())
}

/// Run a single command non-interactively
async fn cmd_run(
    config: &crate::config::Config,
    prompt: String,
    auto_approve: bool,
    format: OutputFormat,
) -> Result<()> {
    use crate::{
        db::Database,
        llm::{
            agent::AgentService,
            tools::{
                bash::BashTool, code_exec::CodeExecTool, context::ContextTool,
                doc_parser::DocParserTool, edit::EditTool, glob::GlobTool, grep::GrepTool,
                http::HttpClientTool, ls::LsTool, notebook::NotebookEditTool, plan_tool::PlanTool,
                read::ReadTool, registry::ToolRegistry, task::TaskTool, web_search::WebSearchTool,
                write::WriteTool,
            },
        },
        services::{ServiceContext, SessionService},
    };

    tracing::info!("Running non-interactive command: {}", prompt);

    // Initialize database
    let db = Database::connect(&config.database.path).await?;
    db.run_migrations().await?;

    // Select provider based on configuration using factory
    let provider = crate::llm::provider::create_provider(config)?;

    // Create tool registry
    let mut tool_registry = ToolRegistry::new();
    // Phase 1: Essential file operations
    tool_registry.register(Arc::new(ReadTool));
    tool_registry.register(Arc::new(WriteTool));
    tool_registry.register(Arc::new(EditTool));
    tool_registry.register(Arc::new(BashTool));
    tool_registry.register(Arc::new(LsTool));
    tool_registry.register(Arc::new(GlobTool));
    tool_registry.register(Arc::new(GrepTool));
    // Phase 2: Advanced features
    tool_registry.register(Arc::new(WebSearchTool));
    tool_registry.register(Arc::new(CodeExecTool));
    tool_registry.register(Arc::new(NotebookEditTool));
    tool_registry.register(Arc::new(DocParserTool));
    // Phase 3: Workflow & integration
    tool_registry.register(Arc::new(TaskTool));
    tool_registry.register(Arc::new(ContextTool));
    tool_registry.register(Arc::new(HttpClientTool));
    tool_registry.register(Arc::new(PlanTool));

    // Create service context and agent service
    let service_context = ServiceContext::new(db.pool().clone());
    let agent_service = AgentService::new(provider.clone(), service_context.clone())
        .with_tool_registry(Arc::new(tool_registry))
        .with_system_prompt(SYSTEM_PROMPT.to_string())
        .with_max_tool_iterations(20);

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

/// Keyring management commands
async fn cmd_keyring(operation: KeyringCommands) -> Result<()> {
    use crate::config::secrets::SecretString;

    match operation {
        KeyringCommands::Set { provider, api_key } => {
            println!("🔐 Saving API key for {} to OS keyring...\n", provider);

            let secret = SecretString::from_str(&api_key);
            let key_name = format!("{}_api_key", provider.to_lowercase());

            secret
                .save_to_keyring(&key_name)
                .with_context(|| format!("Failed to save {} API key to keyring", provider))?;

            println!("✅ Successfully saved {} API key to OS keyring", provider);
            println!("\n💡 The key is now securely stored in your system's credential manager:");
            #[cfg(target_os = "windows")]
            println!("   - Windows Credential Manager");
            #[cfg(target_os = "macos")]
            println!("   - macOS Keychain");
            #[cfg(target_os = "linux")]
            println!("   - Linux Secret Service");

            println!("\n🔒 Security benefits:");
            println!("   ✓ Encrypted by the operating system");
            println!("   ✓ Not stored in plaintext files");
            println!("   ✓ Automatically cleared from memory");

            Ok(())
        }

        KeyringCommands::Get { provider } => {
            let key_name = format!("{}_api_key", provider.to_lowercase());

            match SecretString::from_keyring_optional(&key_name) {
                Some(secret) => {
                    println!("🔐 API key for {}: {}", provider, secret.expose_secret());
                    println!(
                        "\n⚠️  Warning: API key displayed in plain text. Clear your terminal history."
                    );
                }
                None => {
                    println!("❌ No API key found for {} in OS keyring", provider);
                    println!("\n💡 To store an API key, use:");
                    println!("   opencrab keyring set {} YOUR_API_KEY", provider);
                }
            }

            Ok(())
        }

        KeyringCommands::Delete { provider } => {
            let key_name = format!("{}_api_key", provider.to_lowercase());

            SecretString::delete_from_keyring(&key_name)
                .with_context(|| format!("Failed to delete {} API key from keyring", provider))?;

            println!("✅ Deleted {} API key from OS keyring", provider);
            Ok(())
        }

        KeyringCommands::List => {
            println!("🔐 API Keys in OS Keyring\n");

            let providers = ["anthropic", "openai", "gemini", "azure"];
            let mut found_any = false;

            for provider in &providers {
                let key_name = format!("{}_api_key", provider);
                if let Some(secret) = SecretString::from_keyring_optional(&key_name) {
                    let masked = format!(
                        "{}...{}",
                        &secret.expose_secret()[..4.min(secret.len())],
                        if secret.len() > 8 {
                            &secret.expose_secret()[secret.len() - 4..]
                        } else {
                            ""
                        }
                    );
                    println!("  ✓ {:<12} {}", provider, masked);
                    found_any = true;
                } else {
                    println!("  ✗ {:<12} (not configured)", provider);
                }
            }

            if !found_any {
                println!("\n💡 No API keys found in keyring.");
                println!("   To store an API key, use:");
                println!("   opencrab keyring set <provider> <api-key>");
            }

            Ok(())
        }
    }
}

/// Log management commands
async fn cmd_logs(operation: LogCommands) -> Result<()> {
    use crate::logging;
    use std::io::{BufRead, BufReader};

    let log_dir = std::env::current_dir()?.join(".opencrab").join("logs");

    match operation {
        LogCommands::Status => {
            println!("📊 OpenCrab Logging Status\n");
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
                    if path.extension().map(|e| e == "log").unwrap_or(false) {
                        file_count += 1;
                        if let Ok(metadata) = entry.metadata() {
                            total_size += metadata.len();
                            if let Ok(modified) = metadata.modified() {
                                if modified > newest_time {
                                    newest_time = modified;
                                    newest_file = Some(path);
                                }
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
                println!("   opencrab -d");
            } else {
                println!("Status: ❌ No logs found");
                println!("\n💡 To enable debug logging, run with -d flag:");
                println!("   opencrab -d");
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
                println!("💡 Run OpenCrab with -d flag to enable debug logging:");
                println!("   opencrab -d");
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
                println!("\n💡 Run OpenCrab with -d flag to enable debug logging:");
                println!("   opencrab -d");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parse() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
