//! Orchestration: ACP client core, socket/script handling, and the turn loop.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    AuthMethod, AuthenticateRequest, CancelNotification, ClientCapabilities, CloseSessionRequest,
    ContentBlock, CreateTerminalRequest, FileSystemCapabilities, InitializeRequest,
    KillTerminalRequest, ReadTextFileRequest,
};
use agent_client_protocol::util::MatchDispatch;
use agent_client_protocol::{AcpAgent, Agent, Client, ConnectionTo, SessionMessage};
use anyhow::{Context, anyhow, bail};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::cli::{Cli, OutputFormat};
use crate::events::Event;
use crate::permissions;
use crate::pi;
use crate::tool::ToolPlane;

/// The one place the ACP protocol version is pinned (see the version-seam decision).
const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::V1;

/// State shared with the connection handlers.
pub struct Shared {
    /// The parsed CLI options.
    pub cli: Cli,
    /// The tool execution plane.
    pub tool: ToolPlane,
}

/// A command sent by the orchestration script over the socket.
#[derive(Debug, Clone)]
pub enum ScriptCommand {
    /// Send a prompt.
    Prompt {
        /// Optional script-side id.
        id: Option<String>,
        /// Prompt text.
        text: String,
    },
    /// Cancel the in-flight turn.
    Cancel {
        /// Optional script-side id.
        id: Option<String>,
    },
    /// Change a session config option.
    SetConfig {
        /// Option id.
        option: String,
        /// New value.
        value: String,
    },
    /// Stop the run.
    Close,
}

#[derive(serde::Deserialize)]
struct WireCommand {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
    id: Option<String>,
    option: Option<String>,
    value: Option<serde_json::Value>,
}

fn parse_command(line: &str) -> Option<ScriptCommand> {
    let msg: WireCommand = serde_json::from_str(line.trim()).ok()?;
    match msg.kind.as_str() {
        "prompt" => msg
            .text
            .map(|text| ScriptCommand::Prompt { id: msg.id, text }),
        "cancel" => Some(ScriptCommand::Cancel { id: msg.id }),
        "set_config" => match (msg.option, msg.value) {
            (Some(option), Some(value)) => Some(ScriptCommand::SetConfig {
                option,
                value: value_to_string(value),
            }),
            _ => None,
        },
        "close" => Some(ScriptCommand::Close),
        _ => None,
    }
}

fn value_to_string(value: serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s,
        other => other.to_string(),
    }
}

/// Accumulates events for stdout according to `--format`.
struct Output {
    format: OutputFormat,
    session_id: Option<String>,
    text: String,
    tools: Vec<Event>,
    failed: bool,
}

impl Output {
    fn new(format: OutputFormat) -> Self {
        Self {
            format,
            session_id: None,
            text: String::new(),
            tools: Vec::new(),
            failed: false,
        }
    }

    fn on_event(&mut self, event: &Event) {
        if self.format == OutputFormat::Ndjson {
            println!("{}", event.to_line());
        }
        match event {
            Event::Ready { session_id } => self.session_id = Some(session_id.clone()),
            Event::Text { delta } => self.text.push_str(delta),
            Event::Tool { id, .. } => {
                let existing = self
                    .tools
                    .iter_mut()
                    .find(|tool| matches!(tool, Event::Tool { id: other, .. } if other == id));
                match existing {
                    Some(tool) => tool.merge_tool_update(event),
                    None => self.tools.push(event.clone()),
                }
            }
            Event::Error { message } => {
                self.failed = true;
                eprintln!("error: {message}");
            }
            Event::TurnDone { .. } => {}
        }
    }

    fn finish(&self) {
        match self.format {
            OutputFormat::Text => {
                if !self.text.is_empty() {
                    println!("{}", self.text);
                }
            }
            OutputFormat::Json => {
                let doc = serde_json::json!({
                    "session_id": self.session_id,
                    "final_text": self.text,
                    "tool_calls": self.tools,
                    "failed": self.failed,
                });
                println!("{doc}");
            }
            OutputFormat::Ndjson | OutputFormat::Quiet => {}
        }
    }
}

/// Build the agent subprocess handle from the CLI.
fn build_agent(cli: &Cli, pi_dir: Option<String>) -> anyhow::Result<AcpAgent> {
    let base = if let Some(command) = &cli.agent_command {
        command
            .parse::<AcpAgent>()
            .map_err(|e| anyhow!("invalid --agent-command: {e}"))?
    } else {
        let (program, args) = pi::registry_command(&cli.agent)
            .ok_or_else(|| anyhow!("unknown agent `{}`; use --agent-command", cli.agent))?;
        let mut argv = vec![program.to_string()];
        argv.extend(args.into_iter().map(str::to_string));
        AcpAgent::from_args(argv).map_err(|e| anyhow!("failed to build agent command: {e}"))?
    };
    Ok(match pi_dir {
        Some(dir) => AcpAgent::new(base.into_config().env("PI_CODING_AGENT_DIR", dir)),
        None => base,
    })
}

fn credentials_available(method_id: &str) -> bool {
    let key = method_id.to_ascii_uppercase().replace(['-', ' '], "_");
    let candidates = [
        format!("OCTX_TOKEN_{key}"),
        format!("{key}_API_KEY"),
        format!("{key}_TOKEN"),
    ];
    candidates
        .iter()
        .any(|name| std::env::var(name).map(|v| !v.is_empty()).unwrap_or(false))
        || std::env::vars().any(|(k, v)| k.starts_with("OCTX_TOKEN_") && !v.is_empty())
}

fn stop_reason_name(reason: &agent_client_protocol::schema::v1::StopReason) -> String {
    use agent_client_protocol::schema::v1::StopReason;
    match reason {
        StopReason::EndTurn => "end_turn",
        StopReason::MaxTokens => "max_tokens",
        StopReason::MaxTurnRequests => "max_turn_requests",
        StopReason::Refusal => "refusal",
        StopReason::Cancelled => "cancelled",
        other => return format!("{other:?}").to_ascii_lowercase(),
    }
    .to_string()
}

/// Run the agent arm. Returns the process exit code.
pub async fn run(cli: Cli) -> anyhow::Result<i32> {
    let cwd = cli.working_dir();
    let tool = ToolPlane::new(&cwd, cli.output_byte_limit).map_err(|e| anyhow!(e))?;
    let shared = Arc::new(Shared {
        cli: cli.clone(),
        tool,
    });

    let pi_setup = if cli.agent == "pi" && !cli.skills.is_empty() {
        Some(pi::setup_skills(&cli.skills).map_err(|e| anyhow!(e))?)
    } else {
        None
    };
    let agent = build_agent(&cli, pi_setup.as_ref().map(pi::PiSetup::agent_dir))?;

    let script_mode = cli.script.is_some();
    let ipc_path = cli.ipc_path.clone().unwrap_or_else(|| {
        std::env::temp_dir().join(format!("octx-agent-{}.sock", std::process::id()))
    });

    let (event_tx, event_rx) = unbounded_channel::<Event>();
    let (line_tx, line_rx) = unbounded_channel::<String>();
    let (prompt_tx, prompt_rx) = unbounded_channel::<ScriptCommand>();

    let listener = if script_mode {
        let _ = std::fs::remove_file(&ipc_path);
        Some(
            UnixListener::bind(&ipc_path)
                .with_context(|| format!("binding {}", ipc_path.display()))?,
        )
    } else {
        None
    };

    let output = Arc::new(std::sync::Mutex::new(Output::new(cli.format)));
    let pump_out = Arc::clone(&output);
    let pump_lines = if script_mode { Some(line_tx) } else { None };
    let output_pump = tokio::spawn(async move {
        let mut rx = event_rx;
        while let Some(event) = rx.recv().await {
            {
                let mut out = pump_out.lock().expect("output poisoned");
                out.on_event(&event);
            }
            if let Some(tx) = &pump_lines {
                let _ = tx.send(event.to_line());
            }
        }
        pump_out.lock().expect("output poisoned").finish();
    });

    if let Some(listener) = listener {
        tokio::spawn(async move {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let (read_half, mut write_half) = stream.into_split();
            let mut lines = BufReader::new(read_half).lines();
            let mut out_rx = line_rx;
            loop {
                tokio::select! {
                    line = lines.next_line() => match line {
                        Ok(Some(line)) => {
                            if let Some(cmd) = parse_command(&line)
                                && prompt_tx.send(cmd).is_err()
                            {
                                break;
                            }
                        }
                        _ => break,
                    },
                    out = out_rx.recv() => match out {
                        Some(line) => {
                            if write_half.write_all(line.as_bytes()).await.is_err()
                                || write_half.write_all(b"\n").await.is_err()
                            {
                                break;
                            }
                        }
                        None => break,
                    },
                }
            }
        });
    }

    let mut script_child = match &cli.script {
        Some(script) => {
            let mut cmd = tokio::process::Command::new(script);
            cmd.args(cli.all_script_args());
            cmd.env("HARNESS_SOCKET", ipc_path.display().to_string());
            for kv in &cli.script_env {
                if let Some((k, v)) = kv.split_once('=') {
                    cmd.env(k, v);
                }
            }
            cmd.stdin(std::process::Stdio::null());
            Some(
                cmd.spawn()
                    .with_context(|| format!("spawning script {}", script.display()))?,
            )
        }
        None => None,
    };

    let oneshot = if script_mode {
        None
    } else {
        cli.prompt.clone()
    };
    if !script_mode && oneshot.is_none() {
        bail!("nothing to do: pass --script or --prompt");
    }

    let sh = Arc::clone(&shared);
    let ev = event_tx.clone();
    let connect_result = Client
        .builder()
        .name("octx-agent")
        .on_receive_request(
            {
                let shared = Arc::clone(&shared);
                async move |request: agent_client_protocol::schema::v1::RequestPermissionRequest,
                            responder,
                            _cx| {
                    responder.respond(permissions::decide(shared.cli.permission_mode, &request))
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let shared = Arc::clone(&shared);
                async move |request: ReadTextFileRequest, responder, _cx| match shared
                    .tool
                    .read_file(&request)
                {
                    Ok(response) => responder.respond(response),
                    Err(e) => responder.respond_with_internal_error(e),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let shared = Arc::clone(&shared);
                async move |request: agent_client_protocol::schema::v1::WriteTextFileRequest,
                            responder,
                            _cx| match shared.tool.write_file(&request) {
                    Ok(response) => responder.respond(response),
                    Err(e) => responder.respond_with_internal_error(e),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let shared = Arc::clone(&shared);
                async move |request: CreateTerminalRequest, responder, _cx| match shared
                    .tool
                    .create_terminal(&request)
                    .await
                {
                    Ok(response) => responder.respond(response),
                    Err(e) => responder.respond_with_internal_error(e),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let shared = Arc::clone(&shared);
                async move |request: agent_client_protocol::schema::v1::TerminalOutputRequest,
                            responder,
                            _cx| match shared.tool.terminal_output(&request).await {
                    Ok(response) => responder.respond(response),
                    Err(e) => responder.respond_with_internal_error(e),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let shared = Arc::clone(&shared);
                async move |request: agent_client_protocol::schema::v1::WaitForTerminalExitRequest,
                            responder,
                            _cx| match shared.tool.wait_for_exit(&request).await {
                    Ok(response) => responder.respond(response),
                    Err(e) => responder.respond_with_internal_error(e),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let shared = Arc::clone(&shared);
                async move |request: KillTerminalRequest, responder, _cx| match shared
                    .tool
                    .kill_terminal(&request.terminal_id.0)
                    .await
                {
                    Ok(response) => responder.respond(response),
                    Err(e) => responder.respond_with_internal_error(e),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let shared = Arc::clone(&shared);
                async move |request: agent_client_protocol::schema::v1::ReleaseTerminalRequest,
                            responder,
                            _cx| match shared
                    .tool
                    .release_terminal(&request.terminal_id.0)
                    .await
                {
                    Ok(response) => responder.respond(response),
                    Err(e) => responder.respond_with_internal_error(e),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, move |cx: ConnectionTo<Agent>| async move {
            orchestrate(cx, sh, ev, prompt_rx, oneshot)
                .await
                .map_err(|e| agent_client_protocol::util::internal_error(format!("{e:#}")))
        })
        .await;

    drop(event_tx);
    let _ = output_pump.await;

    let mut exit_code = match connect_result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    };

    if let Some(child) = script_child.as_mut() {
        match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
            Ok(Ok(status)) => {
                if exit_code == 0 {
                    exit_code = status.code().unwrap_or(1);
                }
            }
            _ => {
                let _ = child.kill().await;
                if exit_code == 0 {
                    exit_code = 1;
                }
            }
        }
    }

    if script_mode {
        let _ = std::fs::remove_file(&ipc_path);
    }
    Ok(exit_code)
}

async fn orchestrate(
    cx: ConnectionTo<Agent>,
    shared: Arc<Shared>,
    event_tx: UnboundedSender<Event>,
    mut prompt_rx: UnboundedReceiver<ScriptCommand>,
    oneshot: Option<String>,
) -> anyhow::Result<()> {
    let cli = &shared.cli;
    let emit = |event: Event| {
        let _ = event_tx.send(event);
    };

    // --- Initialize ---
    let mut capabilities = ClientCapabilities::default();
    capabilities.terminal = true;
    capabilities.fs = FileSystemCapabilities::new()
        .read_text_file(true)
        .write_text_file(true);
    let mut init_request = InitializeRequest::new(PROTOCOL_VERSION);
    init_request.client_capabilities = capabilities;
    let init = cx
        .send_request(init_request)
        .block_task()
        .await
        .map_err(|e| anyhow!("initialize failed: {e}"))?;

    // --- Authentication ---
    // Only agent-type methods are satisfied through `authenticate`. Terminal
    // methods mean the client would run the agent interactively, which a
    // headless arm cannot do, so they are a warning rather than a failure.
    if !init.auth_methods.is_empty() {
        let agent_methods: Vec<&AuthMethod> = init
            .auth_methods
            .iter()
            .filter(|method| matches!(method, AuthMethod::Agent(_)))
            .collect();
        if let Some(method) = agent_methods
            .iter()
            .find(|method| credentials_available(&method.id().0))
        {
            cx.send_request(AuthenticateRequest::new(method.id().clone()))
                .block_task()
                .await
                .map_err(|e| anyhow!("authenticate failed: {e}"))?;
        } else if agent_methods.is_empty() {
            let ids: Vec<&str> = init
                .auth_methods
                .iter()
                .map(|method| method.id().0.as_ref())
                .collect();
            eprintln!(
                "warning: agent advertises terminal login methods ({}); continuing — \
                 run the agent's own login if it needs credentials",
                ids.join(", ")
            );
        } else {
            let ids: Vec<&str> = agent_methods
                .iter()
                .map(|method| method.id().0.as_ref())
                .collect();
            bail!(
                "agent requires authentication but no credentials are available; \
                 provide credentials for one of the following auth methods: {}",
                ids.join(", ")
            );
        }
    }

    // --- Session ---
    let cwd: PathBuf = shared.tool.cwd().to_path_buf();
    let mut session = match cli.session.as_deref() {
        Some(id) if !id.is_empty() => {
            let resume_advertised = init
                .agent_capabilities
                .session_capabilities
                .resume
                .is_some();
            if resume_advertised {
                match cx
                    .resume_session(
                        agent_client_protocol::schema::v1::SessionId::new(id.to_string()),
                        &cwd,
                    )
                    .block_task()
                    .start_session()
                    .await
                {
                    Ok(restored) => restored.into_parts().0,
                    Err(e) => {
                        eprintln!(
                            "warning: could not resume session `{id}`: {e}; creating a new session"
                        );
                        cx.build_session(&cwd).block_task().start_session().await?
                    }
                }
            } else {
                eprintln!(
                    "warning: agent does not advertise session resume; creating a new session"
                );
                cx.build_session(&cwd).block_task().start_session().await?
            }
        }
        _ => cx.build_session(&cwd).block_task().start_session().await?,
    };

    // --- Session config options ---
    let options = session.config_options().map(|opts| opts.to_vec());
    if let Some(model) = &cli.model {
        apply_option(
            &cx,
            session.session_id(),
            options.as_deref(),
            "model",
            model,
        )
        .await?;
    }
    if let Some(provider) = &cli.provider {
        apply_option(
            &cx,
            session.session_id(),
            options.as_deref(),
            "provider",
            provider,
        )
        .await?;
    }
    if let Some(thinking) = &cli.thinking {
        apply_option(
            &cx,
            session.session_id(),
            options.as_deref(),
            "thought_level",
            thinking,
        )
        .await?;
    }

    // --- Ready ---
    emit(Event::Ready {
        session_id: session.session_id().0.to_string(),
    });

    let mut system_prompt = cli.system_prompt.clone();
    let max_turns = cli.max_turns.unwrap_or(u32::MAX);
    let turn_timeout = cli.timeout.map(Duration::from_secs);
    let mut turns: u32 = 0;
    let mut active: Option<Instant> = None;

    if let Some(prompt) = oneshot.clone() {
        send_turn(&mut session, &mut system_prompt, prompt)?;
        turns += 1;
        active = Some(Instant::now());
    }

    loop {
        let step = if active.is_some() {
            let deadline =
                turn_timeout.map(|t| tokio::time::Instant::from_std(active.unwrap() + t));
            tokio::select! {
                command = prompt_rx.recv() => Step::Command(command),
                message = session.read_update() => Step::Update(message),
                () = sleep_until_opt(deadline) => Step::TimedOut,
            }
        } else {
            tokio::select! {
                command = prompt_rx.recv() => Step::Command(command),
                else => Step::Closed,
            }
        };

        match step {
            Step::TimedOut => {
                let _ = cx.send_notification(CancelNotification::new(session.session_id().clone()));
                emit(Event::Error {
                    message: "turn timed out".to_string(),
                });
                bail!("turn exceeded --timeout");
            }
            Step::Update(Err(e)) => {
                emit(Event::Error {
                    message: format!("agent subprocess ended: {e}"),
                });
                bail!("agent subprocess ended: {e}");
            }
            Step::Update(Ok(SessionMessage::StopReason(reason))) => {
                emit(Event::TurnDone {
                    stop_reason: stop_reason_name(&reason),
                    usage: None,
                });
                active = None;
                if oneshot.is_some() {
                    break;
                }
            }
            Step::Update(Ok(SessionMessage::SessionMessage(dispatch))) => {
                MatchDispatch::new(dispatch)
                    .if_notification(async |notification: agent_client_protocol::schema::v1::SessionNotification| {
                        handle_update(&notification.update, &event_tx);
                        Ok::<(), agent_client_protocol::Error>(())
                    })
                    .await
                    .otherwise_ignore()?;
            }
            Step::Update(Ok(_)) => {}
            Step::Closed | Step::Command(None) => break,
            Step::Command(Some(command)) => match command {
                ScriptCommand::Close => break,
                ScriptCommand::Cancel { .. } => {
                    let _ =
                        cx.send_notification(CancelNotification::new(session.session_id().clone()));
                }
                ScriptCommand::SetConfig { option, value } => {
                    apply_option(
                        &cx,
                        session.session_id(),
                        options.as_deref(),
                        &option,
                        &value,
                    )
                    .await?;
                }
                ScriptCommand::Prompt { text, .. } => {
                    if turns >= max_turns {
                        let _ = cx.send_notification(CancelNotification::new(
                            session.session_id().clone(),
                        ));
                        emit(Event::Error {
                            message: format!("max turns ({max_turns}) exceeded"),
                        });
                        bail!("max turns ({max_turns}) exceeded");
                    }
                    send_turn(&mut session, &mut system_prompt, text)?;
                    turns += 1;
                    active = Some(Instant::now());
                }
            },
        }
    }

    if init.agent_capabilities.session_capabilities.close.is_some() {
        let _ = cx
            .send_request(CloseSessionRequest::new(session.session_id().clone()))
            .block_task()
            .await;
    }
    Ok(())
}

#[allow(
    clippy::large_enum_variant,
    reason = "SessionMessage is large; boxing every update adds an allocation to the hot path"
)]
enum Step {
    Command(Option<ScriptCommand>),
    Update(Result<SessionMessage, agent_client_protocol::Error>),
    TimedOut,
    Closed,
}

async fn sleep_until_opt(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending::<()>().await,
    }
}

fn send_turn(
    session: &mut agent_client_protocol::ActiveSession<'static, Agent>,
    system_prompt: &mut Option<String>,
    text: String,
) -> anyhow::Result<()> {
    let prompt = match system_prompt.take() {
        Some(prefix) => format!("{prefix}\n\n{text}"),
        None => text,
    };
    session
        .send_prompt(prompt)
        .map_err(|e| anyhow!("failed to send prompt: {e}"))?;
    Ok(())
}

fn handle_update(
    update: &agent_client_protocol::schema::v1::SessionUpdate,
    event_tx: &UnboundedSender<Event>,
) {
    use agent_client_protocol::schema::v1::{SessionUpdate, ToolCallStatus};
    let tool_status = |status: ToolCallStatus| match status {
        ToolCallStatus::Pending => "pending",
        ToolCallStatus::InProgress => "running",
        ToolCallStatus::Completed => "done",
        ToolCallStatus::Failed => "failed",
        _ => "unknown",
    };
    match update {
        SessionUpdate::AgentMessageChunk(chunk) => {
            if let ContentBlock::Text(text) = &chunk.content {
                let _ = event_tx.send(Event::Text {
                    delta: text.text.clone(),
                });
            }
        }
        SessionUpdate::ToolCall(call) => {
            let _ = event_tx.send(Event::Tool {
                name: Some(call.name.clone().unwrap_or_else(|| call.title.clone())),
                status: tool_status(call.status).to_string(),
                id: call.tool_call_id.0.to_string(),
                output: None,
                exit_code: None,
            });
        }
        SessionUpdate::ToolCallUpdate(update) => {
            if let Some(status) = update.fields.status {
                let _ = event_tx.send(Event::Tool {
                    name: update.fields.name.clone(),
                    status: tool_status(status).to_string(),
                    id: update.tool_call_id.0.to_string(),
                    output: None,
                    exit_code: None,
                });
            }
        }
        // Unknown or unrecognised updates are ignored, not fatal.
        _ => {}
    }
}

async fn apply_option(
    cx: &ConnectionTo<Agent>,
    session_id: &agent_client_protocol::schema::v1::SessionId,
    options: Option<&[agent_client_protocol::schema::v1::SessionConfigOption]>,
    wanted: &str,
    value: &str,
) -> anyhow::Result<()> {
    use agent_client_protocol::schema::v1::{
        SessionConfigOptionValue, SessionConfigValueId, SetSessionConfigOptionRequest,
    };
    let Some(options) = options else {
        eprintln!("warning: agent advertises no session config options; ignoring `{wanted}`");
        return Ok(());
    };
    let wanted_lower = wanted.to_ascii_lowercase();
    let found = options.iter().find(|option| {
        let id = option.id.0.to_ascii_lowercase();
        id == wanted_lower || id.contains(&wanted_lower)
    });
    match found {
        Some(option) => {
            cx.send_request(SetSessionConfigOptionRequest::new(
                session_id.clone(),
                option.id.clone(),
                SessionConfigOptionValue::ValueId {
                    value: SessionConfigValueId::new(value.to_string()),
                },
            ))
            .block_task()
            .await
            .map_err(|e| anyhow!("failed to set `{wanted}`: {e}"))?;
        }
        None => eprintln!("warning: agent does not advertise `{wanted}`; ignoring"),
    }
    Ok(())
}
