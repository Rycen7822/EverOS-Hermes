use crate::mcp;
use crate::provider::{EverOSProvider, ProviderInit, provider_tool_schemas, save_config};
use anyhow::{Context, anyhow};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use std::io::{self, Read};
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
    /// Hermes memory-provider helper commands used by the Python shim; short-lived compatibility shim.
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
        payload_stdin: bool,
    },
    SystemPrompt {
        #[arg(long)]
        payload_stdin: bool,
    },
    Prefetch {
        #[arg(long)]
        payload_stdin: bool,
    },
    ToolCall {
        /// Tool name is non-sensitive; tool arguments must be supplied via --payload-stdin.
        #[arg(long)]
        tool_name: String,
        #[arg(long)]
        payload_stdin: bool,
    },
    SyncTurn {
        #[arg(long)]
        payload_stdin: bool,
    },
    OnMemoryWrite {
        #[arg(long)]
        payload_stdin: bool,
    },
    OnSessionEnd {
        #[arg(long)]
        payload_stdin: bool,
    },
    OnPreCompress {
        #[arg(long)]
        payload_stdin: bool,
    },
    OnDelegation {
        #[arg(long)]
        payload_stdin: bool,
    },
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
            payload_stdin,
        } => {
            let payload = read_required_payload(payload_stdin)?;
            let values = payload.get("values").cloned().unwrap_or(payload);
            save_config(&values, &hermes_home)?;
            println!("{}", serde_json::to_string_pretty(&json!({"saved": true}))?);
        }
        ProviderCommand::SystemPrompt { payload_stdin } => {
            let (_, provider) = short_lived_provider_payload(payload_stdin)?;
            println!("{}", provider.system_prompt_block());
        }
        ProviderCommand::Prefetch { payload_stdin } => {
            let (payload, provider) = short_lived_provider_payload(payload_stdin)?;
            let query = payload_string(&payload, "query");
            let session_id_override = payload_string(&payload, "session_id_override");
            let sid = if session_id_override.is_empty() {
                None
            } else {
                Some(session_id_override.as_str())
            };
            println!("{}", provider.prefetch(&query, sid));
        }
        ProviderCommand::ToolCall {
            tool_name,
            payload_stdin,
        } => {
            let (payload, provider) = short_lived_provider_payload(payload_stdin)?;
            let args = payload_value(&payload, "args", json!({}));
            println!("{}", provider.handle_tool_call(&tool_name, args)?);
        }
        ProviderCommand::SyncTurn { payload_stdin } => {
            let (payload, provider) = short_lived_provider_payload(payload_stdin)?;
            let user_content = payload_string(&payload, "user_content");
            let assistant_content = payload_string(&payload, "assistant_content");
            let session_id_override = payload_string(&payload, "session_id_override");
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
        ProviderCommand::OnMemoryWrite { payload_stdin } => {
            let (payload, provider) = short_lived_provider_payload(payload_stdin)?;
            let action = payload_string(&payload, "action");
            let target = payload_string(&payload, "target");
            let content = payload_string(&payload, "content");
            let metadata = match payload.get("metadata") {
                Some(Value::Null) | None => None,
                Some(value) => Some(value.clone()),
            };
            provider.on_memory_write(&action, &target, &content, metadata)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({"queued": true}))?
            );
        }
        ProviderCommand::OnSessionEnd { payload_stdin } => {
            let (payload, provider) = short_lived_provider_payload(payload_stdin)?;
            let messages = payload_messages(&payload)?;
            provider.on_session_end(&messages)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({"flushed": true}))?
            );
        }
        ProviderCommand::OnPreCompress { payload_stdin } => {
            let (payload, provider) = short_lived_provider_payload(payload_stdin)?;
            let messages = payload_messages(&payload)?;
            println!("{}", provider.on_pre_compress(&messages)?);
        }
        ProviderCommand::OnDelegation { payload_stdin } => {
            let (payload, provider) = short_lived_provider_payload(payload_stdin)?;
            let task = payload_string(&payload, "task");
            let result = payload_string(&payload, "result");
            let child_session_id = payload_string(&payload, "child_session_id");
            let child = if child_session_id.is_empty() {
                None
            } else {
                Some(child_session_id.as_str())
            };
            provider.on_delegation(&task, &result, child)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({"captured": true}))?
            );
        }
    }
    Ok(())
}

fn read_required_payload(enabled: bool) -> anyhow::Result<Value> {
    if !enabled {
        return Err(anyhow!(
            "provider helper payloads must be supplied with --payload-stdin"
        ));
    }
    let mut raw = String::new();
    io::stdin()
        .read_to_string(&mut raw)
        .context("failed reading --payload-stdin")?;
    if raw.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(&raw).context("invalid --payload-stdin JSON")
}

// Provider subcommands are one-shot calls from the Python shim: payload in, one provider instance, response out.
fn short_lived_provider_payload(payload_stdin: bool) -> anyhow::Result<(Value, EverOSProvider)> {
    let payload = read_required_payload(payload_stdin)?;
    let provider = short_lived_provider_from_payload(&payload)?;
    Ok((payload, provider))
}

fn payload_string(payload: &Value, key: &str) -> String {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_default()
}

fn payload_value(payload: &Value, key: &str, fallback: Value) -> Value {
    payload.get(key).cloned().unwrap_or(fallback)
}

fn payload_messages(payload: &Value) -> anyhow::Result<Vec<Value>> {
    payload
        .get("messages")
        .unwrap_or(&Value::Array(Vec::new()))
        .as_array()
        .cloned()
        .ok_or_else(|| anyhow!("payload messages must be a JSON array"))
}

fn short_lived_provider_from_payload(payload: &Value) -> anyhow::Result<EverOSProvider> {
    let mut init = ProviderInit::default();
    if let Some(state) = payload.get("state") {
        apply_short_lived_provider_state(&mut init, state)?;
    }
    normalize_short_lived_provider_init(&mut init);
    Ok(EverOSProvider::initialize(init)?)
}

fn apply_short_lived_provider_state(init: &mut ProviderInit, value: &Value) -> anyhow::Result<()> {
    if !value.is_object() {
        return Err(anyhow!("state must be a JSON object"));
    }
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
    normalize_short_lived_provider_init(init);
    Ok(())
}

fn normalize_short_lived_provider_init(init: &mut ProviderInit) {
    if init.platform.trim().is_empty() {
        init.platform = "cli".to_string();
    }
    if init.agent_identity.trim().is_empty() {
        init.agent_identity = "default".to_string();
    }
}
