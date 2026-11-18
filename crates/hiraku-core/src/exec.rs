//! Execução de comandos externos com saída transmitida como eventos `Log`.

use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use crate::error::{Error, Result};
use crate::event::{Event, EventSink};

pub fn run_command(module: &str, argv: &[String], sink: &mut dyn EventSink) -> Result<()> {
    run_command_env(module, argv, &[], sink)
}

/// Como `run_command`, com variáveis de ambiente extras para o processo.
pub fn run_command_env(
    module: &str,
    argv: &[String],
    envs: &[(String, String)],
    sink: &mut dyn EventSink,
) -> Result<()> {
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| Error::Unsupported("comando vazio".to_string()))?;
    let cmd_text = argv.join(" ");

    let mut child = Command::new(program)
        .args(args)
        .envs(envs.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Command {
            cmd: cmd_text.clone(),
            reason: e.to_string(),
        })?;

    // stdout e stderr são lidos em threads para não travar por buffer cheio.
    let (tx, rx) = mpsc::channel::<String>();
    let mut readers = Vec::new();
    if let Some(out) = child.stdout.take() {
        readers.push(spawn_reader(out, tx.clone()));
    }
    if let Some(err) = child.stderr.take() {
        readers.push(spawn_reader(err, tx.clone()));
    }
    drop(tx);

    for line in rx {
        sink.emit(Event::Log {
            module: module.to_string(),
            line,
        });
    }
    for reader in readers {
        let _ = reader.join();
    }

    let status = child.wait().map_err(|e| Error::Command {
        cmd: cmd_text.clone(),
        reason: e.to_string(),
    })?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::Command {
            cmd: cmd_text,
            reason: format!("terminou com {status}"),
        })
    }
}

fn spawn_reader<R: Read + Send + 'static>(reader: R, tx: Sender<String>) -> JoinHandle<()> {
    thread::spawn(move || {
        for line in BufReader::new(reader).lines().map_while(|l| l.ok()) {
            if tx.send(line).is_err() {
                break;
            }
        }
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::event::VecSink;

    #[test]
    fn streams_output_lines_and_reports_success() {
        let mut sink = VecSink::default();
        let argv = vec!["sh".to_string(), "-c".to_string(), "echo oi; echo erro 1>&2".to_string()];
        run_command("m", &argv, &mut sink).unwrap();
        let lines: Vec<&str> = sink
            .0
            .iter()
            .filter_map(|e| match e {
                Event::Log { line, .. } => Some(line.as_str()),
                _ => None,
            })
            .collect();
        assert!(lines.contains(&"oi"));
        assert!(lines.contains(&"erro"));
    }

    #[test]
    fn passes_extra_environment_variables() {
        let mut sink = VecSink::default();
        let argv = vec!["sh".to_string(), "-c".to_string(), "echo $HIRAKU_TESTE".to_string()];
        let envs = vec![("HIRAKU_TESTE".to_string(), "valor-ok".to_string())];
        run_command_env("m", &argv, &envs, &mut sink).unwrap();
        assert!(sink
            .0
            .iter()
            .any(|e| matches!(e, Event::Log { line, .. } if line == "valor-ok")));
    }

    #[test]
    fn nonzero_exit_is_an_error() {
        let mut sink = VecSink::default();
        let argv = vec!["sh".to_string(), "-c".to_string(), "exit 3".to_string()];
        assert!(run_command("m", &argv, &mut sink).is_err());
    }

    #[test]
    fn missing_program_is_an_error() {
        let mut sink = VecSink::default();
        let argv = vec!["hiraku-programa-inexistente".to_string()];
        assert!(run_command("m", &argv, &mut sink).is_err());
    }
}
