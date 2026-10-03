use iced::Subscription;
use iced::futures::Stream;
use iced::futures::sink::SinkExt;
use iced::stream;

use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, Command};
use tokio::sync::mpsc::{self, Receiver, error::TryRecvError};

use std::hash::{Hash, Hasher};
use std::time::Duration;
use tokio::time::{Instant, timeout};

use crate::Message;

/// Extracts the relevant fields from one UCI `info` line without trusting
/// engine stdout. `info string` payloads are free-form text, not UCI data.
fn parse_uci_info_line(line: &str) -> (Option<String>, Option<String>) {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.first() == Some(&"info") && tokens.get(1) == Some(&"string") {
        return (None, None);
    }

    let mut eval = None;
    let mut best_move = None;

    for (index, token) in tokens.iter().enumerate() {
        match *token {
            "score" => {
                let score_type = tokens.get(index + 1);
                let score_value = tokens.get(index + 2);
                let parsed_score = score_value.and_then(|value| value.parse::<i32>().ok());

                match (score_type, parsed_score) {
                    (Some(&"cp"), Some(value)) => {
                        eval = Some(format!("{:.2}", value as f32 / 100.0))
                    }
                    (Some(&"mate"), Some(value)) => eval = Some(format!("Mate in {value}")),
                    _ => {}
                }
            }
            "pv" => {
                if let Some(candidate) = tokens.get(index + 1)
                    && crate::puzzles::parse_uci_move(candidate).is_ok()
                {
                    best_move = Some((*candidate).to_string());
                    break;
                }
            }
            _ => {}
        }
    }

    (eval, best_move)
}

#[cfg(test)]
mod tests {
    use super::{
        Engine, EngineRequest, parse_uci_info_line, read_analysis_output,
        wait_for_running_search_boundary,
    };
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    fn subscription_identity_hash(engine: &Engine) -> u64 {
        let mut hasher = DefaultHasher::new();
        engine.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn parses_positive_and_negative_centipawn_scores() {
        assert_eq!(
            parse_uci_info_line("info depth 12 score cp 35 nodes 100"),
            (Some(String::from("0.35")), None)
        );
        assert_eq!(
            parse_uci_info_line("info depth 12 score cp -127 nodes 100"),
            (Some(String::from("-1.27")), None)
        );
    }

    #[test]
    fn parses_mate_scores() {
        assert_eq!(
            parse_uci_info_line("info score mate 3"),
            (Some(String::from("Mate in 3")), None)
        );
        assert_eq!(
            parse_uci_info_line("info score mate -2"),
            (Some(String::from("Mate in -2")), None)
        );
        assert_eq!(
            parse_uci_info_line("info score mate 0"),
            (Some(String::from("Mate in 0")), None)
        );
    }

    #[test]
    fn parses_scores_followed_by_bound_tokens() {
        assert_eq!(
            parse_uci_info_line("info score cp 35 lowerbound nodes 100"),
            (Some(String::from("0.35")), None)
        );
        assert_eq!(
            parse_uci_info_line("info score cp -127 upperbound nps 100"),
            (Some(String::from("-1.27")), None)
        );
    }

    #[test]
    fn ignores_incomplete_or_invalid_scores() {
        for line in [
            "info score",
            "info score cp",
            "info score mate",
            "info score cp not-a-number",
            "info score unknown 35",
            "info depth 12 nodes 100",
        ] {
            assert_eq!(parse_uci_info_line(line).0, None, "{line}");
        }
    }

    #[test]
    fn ignores_free_form_info_string_lines() {
        assert_eq!(
            parse_uci_info_line("info string score cp 300 pv e2e4"),
            (None, None)
        );
        assert_eq!(parse_uci_info_line("info string pv e2e4"), (None, None));
    }

    #[test]
    fn ignores_missing_and_malformed_pv_moves() {
        for line in [
            "info pv",
            "info pv e2",
            "info pv i1i2",
            "info pv e2e4x",
            "info pv e7e8k",
            "info pv a1☃2",
        ] {
            assert_eq!(parse_uci_info_line(line).1, None, "{line}");
        }
    }

    #[test]
    fn accepts_normal_and_promotion_pv_moves() {
        assert_eq!(
            parse_uci_info_line("info depth 12 pv e2e4 e7e5").1,
            Some(String::from("e2e4"))
        );
        assert_eq!(
            parse_uci_info_line("info pv e7e8q").1,
            Some(String::from("e7e8q"))
        );
    }

    #[test]
    fn parses_score_and_pv_from_a_normal_uci_line() {
        assert_eq!(
            parse_uci_info_line("info depth 20 score cp 35 lowerbound nodes 100 pv e2e4 e7e5"),
            (Some(String::from("0.35")), Some(String::from("e2e4")))
        );
    }

    #[test]
    fn engine_subscription_identity_ignores_current_request() {
        let mut first = Engine::new(
            Some(String::from("stockfish-a")),
            String::from("movetime 100"),
            String::from("8/8/8/8/8/8/8/K6k w - - 0 1"),
        );
        let mut second = first.clone();
        second.current_request =
            EngineRequest::new(42, String::from("8/8/8/8/8/8/P7/K6k w - - 0 1"));

        assert_eq!(
            subscription_identity_hash(&first),
            subscription_identity_hash(&second)
        );

        first.engine_path = String::from("stockfish-b");
        assert_ne!(
            subscription_identity_hash(&first),
            subscription_identity_hash(&second)
        );
        first.engine_path = second.engine_path.clone();
        first.search_up_to = String::from("depth 8");
        assert_ne!(
            subscription_identity_hash(&first),
            subscription_identity_hash(&second)
        );
    }

    #[test]
    fn active_search_boundary_discards_late_old_output_before_new_search_output() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("test runtime should start");

        runtime.block_on(async {
            let stdout = concat!(
                "info depth 20 score cp 125 pv e2e4\n",
                "bestmove e2e4\n",
                "info depth 1 score cp -20 pv e7e5\n",
            );
            let (mut writer, reader) = tokio::io::duplex(1024);
            writer
                .write_all(stdout.as_bytes())
                .await
                .expect("synthetic engine output should be written");
            let mut reader = BufReader::new(reader).lines();
            let mut active_search_running = true;

            wait_for_running_search_boundary(&mut reader, &mut active_search_running)
                .await
                .expect("old search should end at bestmove");
            assert!(!active_search_running);

            let mut eval = None;
            let mut best_move = None;
            let completed = read_analysis_output(&mut reader, &mut eval, &mut best_move)
                .await
                .expect("new search output should be readable");

            assert!(!completed);
            assert_eq!(eval, Some(String::from("-0.20")));
            assert_eq!(best_move, Some(String::from("e7e5")));
        });
    }

    #[test]
    fn completed_search_does_not_wait_for_a_second_bestmove_before_new_output() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("test runtime should start");

        runtime.block_on(async {
            let stdout = concat!("bestmove e2e4\n", "info depth 1 score cp -20 pv e7e5\n",);
            let (mut writer, reader) = tokio::io::duplex(1024);
            writer
                .write_all(stdout.as_bytes())
                .await
                .expect("synthetic engine output should be written");
            let mut reader = BufReader::new(reader).lines();
            let mut active_search_running = true;

            let mut eval = None;
            let mut best_move = None;
            let completed = read_analysis_output(&mut reader, &mut eval, &mut best_move)
                .await
                .expect("natural bestmove should be consumed");
            if completed {
                active_search_running = false;
            }

            wait_for_running_search_boundary(&mut reader, &mut active_search_running)
                .await
                .expect("completed search should not wait for another bestmove");

            let mut eval = None;
            let mut best_move = None;
            let completed = read_analysis_output(&mut reader, &mut eval, &mut best_move)
                .await
                .expect("new search output should still be readable");

            assert!(!completed);
            assert_eq!(eval, Some(String::from("-0.20")));
            assert_eq!(best_move, Some(String::from("e7e5")));
        });
    }
}
#[allow(
    clippy::large_enum_variant,
    reason = "The engine state owns process resources; boxing would change lifecycle ownership."
)]
pub enum EngineState {
    Start,
    Thinking(
        Child,
        Lines<BufReader<tokio::process::ChildStdout>>,
        String,
        Receiver<EngineCommand>,
        EngineRequest,
        bool,
    ),
}

#[derive(PartialEq)]
pub enum EngineStatus {
    Started,
    TurnedOff,
}

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct EngineRequest {
    pub generation: u64,
    pub fen: String,
}

impl EngineRequest {
    pub fn new(generation: u64, fen: String) -> Self {
        Self { generation, fen }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EngineUpdate {
    pub generation: u64,
    pub eval: Option<String>,
    pub best_move: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum EngineCommand {
    Search(EngineRequest),
    Stop,
    Exit,
}

#[derive(Debug, Clone)]
pub struct Engine {
    pub engine_path: String,
    pub search_up_to: String,
    pub current_request: EngineRequest,
}

impl Hash for Engine {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.engine_path.hash(state);
        self.search_up_to.hash(state);
    }
}

impl Engine {
    pub fn new(path: Option<String>, limit: String, position: String) -> Self {
        Self {
            engine_path: path.unwrap_or_default(),
            search_up_to: limit,
            current_request: EngineRequest::new(0, position),
        }
    }

    pub fn run_engine(self) -> Subscription<Message> {
        Subscription::run_with(self, Engine::engine_stream)
    }

    fn engine_stream(engine: &Engine) -> impl Stream<Item = Message> + use<> {
        let engine = engine.clone();
        stream::channel(100, async move |mut output| {
            let mut state = EngineState::Start;

            loop {
                match &mut state {
                    EngineState::Start => {
                        let (sender, receiver) = mpsc::channel(100);
                        let mut cmd = Command::new(engine.engine_path.clone());
                        cmd.kill_on_drop(true)
                            .stdin(Stdio::piped())
                            .stdout(Stdio::piped());
                        #[cfg(target_os = "windows")]
                        //"CREATE_NO_WINDOW" flag
                        // https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags
                        cmd.creation_flags(0x08000000);
                        let mut child = match cmd.spawn() {
                            Ok(child) => child,
                            Err(error) => {
                                if output
                                    .send(Message::EngineFailed {
                                        reason: format!("could not start engine: {error}"),
                                        exit_requested: false,
                                    })
                                    .await
                                    .is_err()
                                {
                                    return;
                                }
                                return;
                            }
                        };
                        let active_request = engine.current_request.clone();
                        let pos = String::from("position fen ")
                            + &active_request.fen
                            + &String::from("\n");
                        let limit = String::from("go ") + &engine.search_up_to + "\n";
                        let stdout = match child.stdout.take() {
                            Some(stdout) => stdout,
                            None => {
                                let reason = with_shutdown_result(
                                    &mut child,
                                    String::from("engine stdout is unavailable"),
                                )
                                .await;
                                if output
                                    .send(Message::EngineFailed {
                                        reason,
                                        exit_requested: false,
                                    })
                                    .await
                                    .is_err()
                                {
                                    return;
                                }
                                return;
                            }
                        };
                        let mut reader = BufReader::new(stdout).lines();
                        let startup_result = async {
                            write_engine_command(&mut child, b"uci\n", "sending uci").await?;
                            wait_for_token(&mut reader, "uciok", "uciok").await?;
                            write_engine_command(&mut child, b"ucinewgame\n", "sending ucinewgame")
                                .await?;
                            write_engine_command(&mut child, b"isready\n", "sending isready")
                                .await?;
                            wait_for_token(&mut reader, "readyok", "readyok").await?;
                            write_engine_command(
                                &mut child,
                                b"setoption name UCI_AnalyseMode value true\n",
                                "configuring analysis mode",
                            )
                            .await?;
                            write_engine_command(&mut child, pos.as_bytes(), "sending position")
                                .await?;
                            write_engine_command(&mut child, limit.as_bytes(), "starting analysis")
                                .await
                        }
                        .await;

                        if let Err(reason) = startup_result {
                            let reason = with_shutdown_result(&mut child, reason).await;
                            if output
                                .send(Message::EngineFailed {
                                    reason,
                                    exit_requested: false,
                                })
                                .await
                                .is_err()
                            {
                                return;
                            }
                            return;
                        }

                        if output
                            .send(Message::EngineReady {
                                generation: active_request.generation,
                                sender,
                            })
                            .await
                            .is_err()
                        {
                            return;
                        }
                        state = EngineState::Thinking(
                            child,
                            reader,
                            engine.search_up_to.to_string(),
                            receiver,
                            active_request,
                            true,
                        );
                        continue;
                    }
                    EngineState::Thinking(
                        child,
                        reader,
                        search_up_to,
                        receiver,
                        active_request,
                        active_search_running,
                    ) => {
                        let msg = match receiver.try_recv() {
                            Ok(message) => Some(message),
                            Err(TryRecvError::Empty) => None,
                            Err(TryRecvError::Disconnected) => {
                                let reason = with_shutdown_result(
                                    child,
                                    String::from("engine control channel disconnected"),
                                )
                                .await;
                                if output
                                    .send(Message::EngineFailed {
                                        reason,
                                        exit_requested: false,
                                    })
                                    .await
                                    .is_err()
                                {
                                    return;
                                }
                                return;
                            }
                        };
                        if let Some(msg) = msg {
                            if matches!(msg, EngineCommand::Stop | EngineCommand::Exit) {
                                let exit_requested = matches!(msg, EngineCommand::Exit);
                                match shutdown_engine(child).await {
                                    Ok(()) => {
                                        if output
                                            .send(Message::EngineStopped(exit_requested))
                                            .await
                                            .is_err()
                                        {
                                            return;
                                        }
                                    }
                                    Err(reason) => {
                                        if output
                                            .send(Message::EngineFailed {
                                                reason,
                                                exit_requested,
                                            })
                                            .await
                                            .is_err()
                                        {
                                            return;
                                        }
                                    }
                                }
                                return;
                            } else if let EngineCommand::Search(request) = msg {
                                let pos = String::from("position fen ")
                                    + &request.fen
                                    + &String::from("\n");
                                let limit = String::from("go ") + search_up_to + "\n";
                                let position_result = async {
                                    if *active_search_running {
                                        write_engine_command(
                                            child,
                                            b"stop\n",
                                            "stopping previous analysis",
                                        )
                                        .await?;
                                    }
                                    wait_for_running_search_boundary(reader, active_search_running)
                                        .await?;
                                    write_engine_command(child, pos.as_bytes(), "sending position")
                                        .await?;
                                    write_engine_command(
                                        child,
                                        limit.as_bytes(),
                                        "starting analysis",
                                    )
                                    .await?;
                                    *active_request = request;
                                    *active_search_running = true;
                                    Ok::<(), String>(())
                                }
                                .await;
                                if let Err(reason) = position_result {
                                    let reason = with_shutdown_result(child, reason).await;
                                    if output
                                        .send(Message::EngineFailed {
                                            reason,
                                            exit_requested: false,
                                        })
                                        .await
                                        .is_err()
                                    {
                                        return;
                                    }
                                    return;
                                }
                            }
                        }
                        let mut eval = None;
                        let mut best_move = None;

                        let read_result =
                            read_analysis_output(reader, &mut eval, &mut best_move).await;
                        let search_completed = match read_result {
                            Ok(search_completed) => search_completed,
                            Err(reason) => {
                                let reason = with_shutdown_result(child, reason).await;
                                if output
                                    .send(Message::EngineFailed {
                                        reason,
                                        exit_requested: false,
                                    })
                                    .await
                                    .is_err()
                                {
                                    return;
                                }
                                return;
                            }
                        };
                        if search_completed {
                            *active_search_running = false;
                        }
                        match output.try_send(Message::UpdateEval(EngineUpdate {
                            generation: active_request.generation,
                            eval,
                            best_move,
                        })) {
                            Ok(()) => {}
                            Err(error) if error.is_full() => {}
                            Err(_) => return,
                        }
                    }
                }
            }
        })
    }
}

async fn write_engine_command(
    child: &mut Child,
    command: &[u8],
    action: &str,
) -> Result<(), String> {
    let stdin = child
        .stdin
        .as_mut()
        .ok_or_else(|| String::from("engine stdin is unavailable"))?;
    stdin
        .write_all(command)
        .await
        .map_err(|error| format!("failed while {action}: {error}"))
}

async fn wait_for_token(
    reader: &mut Lines<BufReader<impl AsyncRead + Unpin>>,
    token: &str,
    token_name: &str,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(7);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(format!("timed out waiting for {token_name}"));
        }
        match timeout(remaining, reader.next_line()).await {
            Err(_) => return Err(format!("timed out waiting for {token_name}")),
            Ok(Ok(None)) => return Err(format!("engine stdout closed before {token_name}")),
            Ok(Ok(Some(line))) if line.contains(token) => return Ok(()),
            Ok(Ok(Some(_))) => {}
            Ok(Err(error)) => {
                return Err(format!("failed while waiting for {token_name}: {error}"));
            }
        }
    }
}

const MAX_ANALYSIS_LINES_PER_CYCLE: usize = 32;
const ANALYSIS_READ_BUDGET: Duration = Duration::from_millis(50);
const STOPPED_SEARCH_BOUNDARY_TIMEOUT: Duration = Duration::from_millis(1000);

async fn wait_for_stopped_search_boundary(
    reader: &mut Lines<BufReader<impl AsyncRead + Unpin>>,
) -> Result<(), String> {
    let deadline = Instant::now() + STOPPED_SEARCH_BOUNDARY_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(String::from(
                "timed out waiting for stopped search bestmove",
            ));
        }
        match timeout(remaining, reader.next_line()).await {
            Err(_) => {
                return Err(String::from(
                    "timed out waiting for stopped search bestmove",
                ));
            }
            Ok(Ok(None)) => {
                return Err(String::from(
                    "engine stdout closed before stopped search bestmove",
                ));
            }
            Ok(Ok(Some(line))) if line.starts_with("bestmove") => return Ok(()),
            Ok(Ok(Some(_))) => {}
            Ok(Err(error)) => {
                return Err(format!(
                    "failed while waiting for stopped search bestmove: {error}"
                ));
            }
        }
    }
}

async fn wait_for_running_search_boundary(
    reader: &mut Lines<BufReader<impl AsyncRead + Unpin>>,
    active_search_running: &mut bool,
) -> Result<(), String> {
    if !*active_search_running {
        return Ok(());
    }
    wait_for_stopped_search_boundary(reader).await?;
    *active_search_running = false;
    Ok(())
}

async fn read_analysis_output(
    reader: &mut Lines<BufReader<impl AsyncRead + Unpin>>,
    eval: &mut Option<String>,
    best_move: &mut Option<String>,
) -> Result<bool, String> {
    let deadline = Instant::now() + ANALYSIS_READ_BUDGET;
    for _ in 0..MAX_ANALYSIS_LINES_PER_CYCLE {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(false);
        }
        match timeout(remaining, reader.next_line()).await {
            Err(_) => return Ok(false),
            Ok(Ok(None)) => return Err(String::from("engine stdout closed during analysis")),
            Ok(Ok(Some(line))) => {
                if line.starts_with("bestmove") {
                    return Ok(true);
                }
                let (line_eval, line_best_move) = parse_uci_info_line(&line);
                if line_eval.is_some() {
                    *eval = line_eval;
                }
                if line_best_move.is_some() {
                    *best_move = line_best_move;
                }
            }
            Ok(Err(error)) => return Err(format!("failed while reading engine analysis: {error}")),
        }
    }
    Ok(false)
}

async fn with_shutdown_result(child: &mut Child, reason: String) -> String {
    match shutdown_engine(child).await {
        Ok(()) => reason,
        Err(shutdown_reason) => format!("{reason}; shutdown failed: {shutdown_reason}"),
    }
}

async fn shutdown_engine(child: &mut Child) -> Result<(), String> {
    let mut errors = Vec::new();
    if let Err(error) = write_engine_command(child, b"stop\n", "sending stop").await {
        errors.push(error);
    }
    if let Err(error) = write_engine_command(child, b"quit\n", "sending quit").await {
        errors.push(error);
    }
    match timeout(Duration::from_millis(1000), child.wait()).await {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => errors.push(format!("failed while waiting for engine exit: {error}")),
        Err(_) => match timeout(Duration::from_millis(500), child.kill()).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => errors.push(format!("failed while killing engine: {error}")),
            Err(_) => errors.push(String::from("timed out while killing engine")),
        },
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}
