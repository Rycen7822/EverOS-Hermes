use crate::mcp;
use crate::provider::{EverOSProvider, ProviderInit, provider_tool_schemas, save_config};
use anyhow::Context;
use clap::{Args, Parser, Subcommand};
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "everos-hermes-rust",
    version,
    about = "Rust EverOS MCP server and Hermes memory provider core"
)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Run the local stdio MCP server.
    Mcp,
    /// Hermes memory-provider helper commands used by the Python shim.
    Provider {
        #[command(subcommand)]
        command: Box<ProviderCommand>,
    },
}

#[derive(Debug, Subcommand)]
enum ProviderCommand {
    IsAvailable {
        #[arg(long)]
        hermes_home: Option<PathBuf>,
    },
    ToolSchemas,
    SaveConfig {
        #[arg(long)]
        hermes_home: PathBuf,
        #[arg(long)]
        values_json: String,
    },
    SystemPrompt(StateArgs),
    Prefetch {
        #[command(flatten)]
        state: StateArgs,
        #[arg(long)]
        query: String,
        #[arg(long, default_value = "")]
        session_id_override: String,
    },
    ToolCall {
        #[command(flatten)]
        state: StateArgs,
        #[arg(long)]
        tool_name: String,
        #[arg(long, default_value = "{}")]
        args_json: String,
    },
    SyncTurn {
        #[command(flatten)]
        state: StateArgs,
        #[arg(long)]
        user_content: String,
        #[arg(long)]
        assistant_content: String,
        #[arg(long, default_value = "")]
        session_id_override: String,
    },
    OnMemoryWrite {
        #[command(flatten)]
        state: StateArgs,
        #[arg(long)]
        action: String,
        #[arg(long)]
        target: String,
        #[arg(long)]
        content: String,
        #[arg(long, default_value = "null")]
        metadata_json: String,
    },
    OnSessionEnd(StateArgs),
}

#[derive(Debug, Clone, Args, Default)]
struct StateArgs {
    #[arg(long)]
    state_json: Option<String>,
    #[arg(long, default_value = "")]
    session_id: String,
    #[arg(long)]
    hermes_home: Option<PathBuf>,
    #[arg(long, default_value = "cli")]
    platform: String,
    #[arg(long, default_value = "")]
    user_id: String,
    #[arg(long, default_value = "")]
    user_name: String,
    #[arg(long, default_value = "default")]
    agent_identity: String,
    #[arg(long, default_value = "")]
    agent_context: String,
}

pub fn run() -> anyhow::Result<()> {
    match Cli::parse().command {
        Commands::Mcp => mcp::run_stdio(),
        Commands::Provider { command } => run_provider(*command),
    }
}

fn run_provider(command: ProviderCommand) -> anyhow::Result<()> {
    match command {
        ProviderCommand::IsAvailable { hermes_home } => {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &json!({"available": EverOSProvider::is_available(hermes_home.as_deref())})
                )?
            );
        }
        ProviderCommand::ToolSchemas => {
            println!(
                "{}",
                serde_json::to_string_pretty(&provider_tool_schemas())?
            );
        }
        ProviderCommand::SaveConfig {
            hermes_home,
            values_json,
        } => {
            let values: Value =
                serde_json::from_str(&values_json).context("invalid --values-json")?;
            save_config(&values, &hermes_home)?;
            println!("{}", serde_json::to_string_pretty(&json!({"saved": true}))?);
        }
        ProviderCommand::SystemPrompt(state) => {
            let provider = provider_from_state(state)?;
            println!("{}", provider.system_prompt_block());
        }
        ProviderCommand::Prefetch {
            state,
            query,
            session_id_override,
        } => {
            let provider = provider_from_state(state)?;
            let sid = if session_id_override.is_empty() {
                None
            } else {
                Some(session_id_override.as_str())
            };
            println!("{}", provider.prefetch(&query, sid));
        }
        ProviderCommand::ToolCall {
            state,
            tool_name,
            args_json,
        } => {
            let provider = provider_from_state(state)?;
            let args: Value = serde_json::from_str(&args_json).context("invalid --args-json")?;
            println!("{}", provider.handle_tool_call(&tool_name, args)?);
        }
        ProviderCommand::SyncTurn {
            state,
            user_content,
            assistant_content,
            session_id_override,
        } => {
            let provider = provider_from_state(state)?;
            let sid = if session_id_override.is_empty() {
                None
            } else {
                Some(session_id_override.as_str())
            };
            provider.sync_turn(&user_content, &assistant_content, sid)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({"synced": true}))?
            );
        }
        ProviderCommand::OnMemoryWrite {
            state,
            action,
            target,
            content,
            metadata_json,
        } => {
            let provider = provider_from_state(state)?;
            let metadata = serde_json::from_str::<Value>(&metadata_json).ok();
            provider.on_memory_write(&action, &target, &content, metadata)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({"queued": true}))?
            );
        }
        ProviderCommand::OnSessionEnd(state) => {
            let provider = provider_from_state(state)?;
            provider.on_session_end()?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({"flushed": true}))?
            );
        }
    }
    Ok(())
}

fn provider_from_state(args: StateArgs) -> anyhow::Result<EverOSProvider> {
    let init = provider_init_from_state(args)?;
    Ok(EverOSProvider::initialize(init)?)
}

fn provider_init_from_state(args: StateArgs) -> anyhow::Result<ProviderInit> {
    let mut init = ProviderInit {
        session_id: args.session_id,
        hermes_home: args.hermes_home,
        platform: args.platform,
        user_id: args.user_id,
        user_name: args.user_name,
        agent_identity: args.agent_identity,
        agent_context: args.agent_context,
    };
    if let Some(raw) = args.state_json {
        let value: Value = serde_json::from_str(&raw).context("invalid --state-json")?;
        if let Some(text) = value.get("session_id").and_then(Value::as_str) {
            init.session_id = text.to_string();
        }
        if let Some(text) = value
            .get("hermes_home")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            init.hermes_home = Some(PathBuf::from(text));
        }
        if let Some(text) = value.get("platform").and_then(Value::as_str) {
            init.platform = text.to_string();
        }
        if let Some(text) = value.get("user_id").and_then(Value::as_str) {
            init.user_id = text.to_string();
        }
        if let Some(text) = value.get("user_name").and_then(Value::as_str) {
            init.user_name = text.to_string();
        }
        if let Some(text) = value.get("agent_identity").and_then(Value::as_str) {
            init.agent_identity = text.to_string();
        }
        if let Some(text) = value.get("agent_context").and_then(Value::as_str) {
            init.agent_context = text.to_string();
        }
    }
    if init.platform.trim().is_empty() {
        init.platform = "cli".to_string();
    }
    if init.agent_identity.trim().is_empty() {
        init.agent_identity = "default".to_string();
    }
    Ok(init)
}
