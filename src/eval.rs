use iced::futures::Stream;
use iced::stream;
use iced::futures::sink::SinkExt;
use iced::Subscription;

use std::process::Stdio;
use tokio::sync::mpsc::{self, Receiver};
use tokio::process::{Command, Child};
use tokio::io::{BufReader, AsyncWriteExt, AsyncBufReadExt};

use tokio::time::timeout;
use std::time::Duration;

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
                    (Some(&"cp"), Some(value)) => eval = Some(format!("{:.2}", value as f32 / 100.0)),
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
        assert_eq!(parse_uci_info_line("info depth 12 score cp 35 nodes 100"), (Some(String::from("0.35")), None));
        assert_eq!(parse_uci_info_line("info depth 12 score cp -127 nodes 100"), (Some(String::from("-1.27")), None));
    }

    #[test]
    fn parses_mate_scores() {
        assert_eq!(parse_uci_info_line("info score mate 3"), (Some(String::from("Mate in 3")), None));
        assert_eq!(parse_uci_info_line("info score mate -2"), (Some(String::from("Mate in -2")), None));
        assert_eq!(parse_uci_info_line("info score mate 0"), (Some(String::from("Mate in 0")), None));
    }

    #[test]
    fn parses_scores_followed_by_bound_tokens() {
        assert_eq!(parse_uci_info_line("info score cp 35 lowerbound nodes 100"), (Some(String::from("0.35")), None));
        assert_eq!(parse_uci_info_line("info score cp -127 upperbound nps 100"), (Some(String::from("-1.27")), None));
    }

    #[test]
    fn ignores_incomplete_or_invalid_scores() {
        for line in ["info score", "info score cp", "info score mate", "info score cp not-a-number", "info score unknown 35", "info depth 12 nodes 100"] {
            assert_eq!(parse_uci_info_line(line).0, None, "{line}");
        }
    }

    #[test]
    fn ignores_free_form_info_string_lines() {
        assert_eq!(parse_uci_info_line("info string score cp 300 pv e2e4"), (None, None));
        assert_eq!(parse_uci_info_line("info string pv e2e4"), (None, None));
    }

    #[test]
    fn ignores_missing_and_malformed_pv_moves() {
        for line in ["info pv", "info pv e2", "info pv i1i2", "info pv e2e4x", "info pv e7e8k", "info pv a1☃2"] {
            assert_eq!(parse_uci_info_line(line).1, None, "{line}");
        }
    }

    #[test]
    fn accepts_normal_and_promotion_pv_moves() {
        assert_eq!(parse_uci_info_line("info depth 12 pv e2e4 e7e5").1, Some(String::from("e2e4")));
        assert_eq!(parse_uci_info_line("info pv e7e8q").1, Some(String::from("e7e8q")));
    }

    #[test]
    fn parses_score_and_pv_from_a_normal_uci_line() {
        assert_eq!(parse_uci_info_line("info depth 20 score cp 35 lowerbound nodes 100 pv e2e4 e7e5"), (Some(String::from("0.35")), Some(String::from("e2e4"))));
    }
}
pub enum EngineState {
    Start,
    Thinking(Child, String, Receiver<String>),
    TurnedOff,
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
        Subscription::run_with(
            self,
            Engine::engine_stream,
        )
    }
    
    fn engine_stream(engine: &Engine) -> impl Stream<Item = Message> + use<> {
        let engine = engine.clone();
        stream::channel(100,

            async move |mut output| {

                let mut state = EngineState::Start;

                loop {
                    match &mut state {

                        EngineState::Start => {

                            let (sender, receiver) = mpsc::channel(100);
                            let mut cmd = Command::new(engine.engine_path.clone());
                            cmd.kill_on_drop(true).stdin(Stdio::piped()).stdout(Stdio::piped());
                            #[cfg(target_os = "windows")]
                            //"CREATE_NO_WINDOW" flag
                            // https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags
                            cmd.creation_flags(0x08000000);
                            let mut child = cmd.spawn().expect("Error calling engine");
                            let pos = String::from("position fen ") + &engine.position + &String::from("\n");
                            let limit = String::from("go ") + &engine.search_up_to + "\n";
                            let mut uciok = false;
                            let mut readyok = false;
                            child.stdin.as_mut().unwrap().write_all(b"uci\n").await.expect("Error communicating with engine");
                            let mut reader = BufReader::new(child.stdout.as_mut().unwrap());
                            let mut buf_str = String::new();
                            loop {
                                let uciok_timeout = timeout(Duration::from_millis(7000),
                                    reader.read_line(&mut buf_str)
                                ).await;
                                if uciok_timeout.is_err() {
                                    break;
                                } else if buf_str.contains("uciok") {
                                    uciok = true;
                                    break;
                                }
                            }
                            if uciok {
                                child.stdin.as_mut().unwrap().write_all(b"ucinewgame\n").await.expect("Error communicating with engine");
                                child.stdin.as_mut().unwrap().write_all(b"isready\n").await.expect("Error communicating with engine");
                                buf_str = String::new();
                                loop {
                                    let readyok_timeout = timeout(Duration::from_millis(7000),
                                        reader.read_line(&mut buf_str)
                                    ).await;
                                    if readyok_timeout.is_err() {
                                        break;
                                    } else if buf_str.contains("readyok") {
                                        readyok = true;
                                        break;
                                    }
                                }
                                if readyok {
                                    child.stdin.as_mut().unwrap().write_all(b"setoption name UCI_AnalyseMode value true\n").await.expect("Error communicating with engine");
                                    child.stdin.as_mut().unwrap().write_all(pos.as_bytes()).await.expect("Error communicating with engine");
                                    child.stdin.as_mut().unwrap().write_all(limit.as_bytes()).await.expect("Error communicating with engine");

                                    output.send(Message::EngineReady(sender)).await.expect("Error on the mpsc channel in the engine subscription");
                                    state = EngineState::Thinking(child, engine.search_up_to.to_string(), receiver);
                                    continue;
                                }
                            }
                            eprintln!("Engine took too long to start, aborting...");
                            child.stdin.as_mut().unwrap().write_all(b"stop\n").await.expect("Error communicating with engine");
                            child.stdin.as_mut().unwrap().write_all(b"quit\n").await.expect("Error communicating with engine");
                            let terminate_timeout = timeout(Duration::from_millis(1000),
                                child.wait()
                            ).await;
                            if let Err(e) = terminate_timeout {
                                eprintln!("Error: {e}");
                                eprintln!("Engine didn't quit, killing the process now... ");
                                let kill_result = timeout(Duration::from_millis(500),
                                    child.kill()
                                ).await;
                                if let Err(e) = kill_result {
                                    eprintln!("Error killing the engine process: {e}");
                                }
                            }
                            output.send(Message::EngineStopped(false)).await.expect("Error on the mpsc channel in the engine subscription");
                            state = EngineState::TurnedOff;
                        } EngineState::Thinking(child, search_up_to, receiver) => {
                            let msg = receiver.try_recv();
                            if let Ok(msg) = msg {
                                if msg == STOP_COMMAND || msg == EXIT_APP_COMMAND {
                                    child.stdin.as_mut().unwrap().write_all(b"stop\n").await.expect("Error communicating with engine");
                                    child.stdin.as_mut().unwrap().write_all(b"quit\n").await.expect("Error communicating with engine");
                                    let terminate_timeout = timeout(Duration::from_millis(1000),
                                        child.wait()
                                    ).await;
                                    if let Err(e) = terminate_timeout {
                                        eprintln!("Error: {e}");
                                        eprintln!("Engine didn't quit, killing the process now... ");
                                        let kill_result = timeout(Duration::from_millis(500),
                                            child.kill()
                                        ).await;
                                        if let Err(e) = kill_result {
                                            eprintln!("Error killing the engine process: {e}");
                                        }
                                    }
                                    output.send(Message::EngineStopped(msg == EXIT_APP_COMMAND)).await.expect("Error on the mpsc channel in the engine subscription");
                                    state = EngineState::TurnedOff;
                                    continue;
                                } else {
                                    let pos = String::from("position fen ") + &msg + &String::from("\n");
                                    let limit = String::from("go ") + search_up_to + "\n";
                                    child.stdin.as_mut().unwrap().write_all(b"stop\n").await.expect("Error communicating with engine");
                                    //child.stdin.as_mut().unwrap().write_all(b"setoption name UCI_AnalyseMode value true\n").await.expect("Error communicating with engine");
                                    //child.stdin.as_mut().unwrap().write_all(b"ucinewgame\n").await.expect("Error communicating with engine");
                                    child.stdin.as_mut().unwrap().write_all(pos.as_bytes()).await.expect("Error communicating with engine");
                                    child.stdin.as_mut().unwrap().write_all(limit.as_bytes()).await.expect("Error communicating with engine");
                                }
                            }
                            let mut buf_str = String::new();
                            let mut eval = None;
                            let mut best_move = None;

                            if let Some(out) = child.stdout.as_mut() {
                                let mut reader = BufReader::new(out);
                                loop {
                                    let read_timeout = timeout(Duration::from_millis(50),
                                        reader.read_line(&mut buf_str)
                                    ).await;
                                    if let Ok(Ok(read_result)) = read_timeout {
                                        if read_result == 0 {
                                            break;
                                        }
                                        let (line_eval, line_best_move) = parse_uci_info_line(&buf_str);
                                        if line_eval.is_some() {
                                            eval = line_eval;
                                        }
                                        if line_best_move.is_some() {
                                            best_move = line_best_move;
                                        }
                                        buf_str.clear();
                                    } else {
                                        break;
                                    }
                                }
                            }
                            output.send(Message::UpdateEval((eval, best_move))).await.expect("Error on the mpsc channel in the engine subscription");
                        } EngineState::TurnedOff => {
                            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        }
                    }
                }
            }
        )
    }
}
