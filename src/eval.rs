use iced::Subscription;
use iced::futures::Stream;
use iced::futures::sink::SinkExt;
use iced::stream;

use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdout, Command};
use tokio::sync::mpsc::{self, Receiver, error::TryRecvError};

use std::time::Duration;
use tokio::time::{Instant, timeout};

use crate::Message;

pub const STOP_COMMAND: &str = "STOP";
pub const EXIT_APP_COMMAND: &str = "EXIT";

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
                if let Some(candidate) = tokens.get(index + 1) {
                    if crate::puzzles::parse_uci_move(candidate).is_ok() {
                        best_move = Some((*candidate).to_string());
                        break;
                    }
                }
            }
            _ => {}
        }
    }

    (eval, best_move)
}

#[cfg(test)]
mod tests {
    use super::parse_uci_info_line;

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
}
pub enum EngineState {
    Start,
    Thinking(
        Child,
        Lines<BufReader<ChildStdout>>,
        String,
        Receiver<String>,
    ),
}

#[derive(PartialEq)]
pub enum EngineStatus {
    Started,
    TurnedOff,
}

#[derive(Debug, Clone, Hash)]
pub struct Engine {
    pub engine_path: String,
    pub search_up_to: String,
    pub position: String,
}

impl Engine {
    pub fn new(path: Option<String>, limit: String, position: String) -> Self {
        Self {
            engine_path: path.unwrap_or_default(),
            search_up_to: limit,
            position,
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
                        let pos =
                            String::from("position fen ") + &engine.position + &String::from("\n");
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

                        if output.send(Message::EngineReady(sender)).await.is_err() {
                            return;
                        }
                        state = EngineState::Thinking(
                            child,
                            reader,
                            engine.search_up_to.to_string(),
                            receiver,
                        );
                        continue;
                    }
                    EngineState::Thinking(child, reader, search_up_to, receiver) => {
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
                            if msg == STOP_COMMAND || msg == EXIT_APP_COMMAND {
                                match shutdown_engine(child).await {
                                    Ok(()) => {
                                        if output
                                            .send(Message::EngineStopped(msg == EXIT_APP_COMMAND))
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
                                                exit_requested: msg == EXIT_APP_COMMAND,
                                            })
                                            .await
                                            .is_err()
                                        {
                                            return;
                                        }
                                    }
                                }
                                return;
                            } else {
                                let pos =
                                    String::from("position fen ") + &msg + &String::from("\n");
                                let limit = String::from("go ") + search_up_to + "\n";
                                let position_result = async {
                                    write_engine_command(
                                        child,
                                        b"stop\n",
                                        "stopping previous analysis",
                                    )
                                    .await?;
                                    write_engine_command(child, pos.as_bytes(), "sending position")
                                        .await?;
                                    write_engine_command(
                                        child,
                                        limit.as_bytes(),
                                        "starting analysis",
                                    )
                                    .await
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
                        if let Err(reason) = read_result {
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
                        match output.try_send(Message::UpdateEval((eval, best_move))) {
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
    reader: &mut Lines<BufReader<ChildStdout>>,
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

async fn read_analysis_output(
    reader: &mut Lines<BufReader<ChildStdout>>,
    eval: &mut Option<String>,
    best_move: &mut Option<String>,
) -> Result<(), String> {
    let deadline = Instant::now() + ANALYSIS_READ_BUDGET;
    for _ in 0..MAX_ANALYSIS_LINES_PER_CYCLE {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(());
        }
        match timeout(remaining, reader.next_line()).await {
            Err(_) => return Ok(()),
            Ok(Ok(None)) => return Err(String::from("engine stdout closed during analysis")),
            Ok(Ok(Some(line))) => {
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
    Ok(())
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
