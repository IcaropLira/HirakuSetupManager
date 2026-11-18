//! Obtenção de privilégio de root. Cada frontend implementa a sua estratégia.
//!
//! `SudoElevator` roda `sudo -v` com o terminal herdado: a TUI precisa ser
//! suspensa (sair do modo raw) ANTES de chamar `ensure_root`, e retomada depois.
//! O app nunca vê a senha; só reaproveita a credencial em cache do sudo.

use std::process::Command;

use crate::error::{Error, Result};

pub trait Elevator {
    fn ensure_root(&mut self) -> Result<()>;
}

/// Para quando já se é root, ou no modo --user.
pub struct NoElevation;

impl Elevator for NoElevation {
    fn ensure_root(&mut self) -> Result<()> {
        Ok(())
    }
}

pub struct SudoElevator;

impl Elevator for SudoElevator {
    fn ensure_root(&mut self) -> Result<()> {
        let status = Command::new("sudo")
            .arg("-v")
            .status()
            .map_err(|e| Error::Elevation(format!("não consegui executar sudo: {e}")))?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::Elevation(format!("`sudo -v` terminou com {status}")))
        }
    }
}

/// Para execuções agendadas: só funciona com sudo sem senha (ou credencial em cache).
/// Nunca bloqueia esperando senha.
pub struct NonInteractiveSudo;

impl Elevator for NonInteractiveSudo {
    fn ensure_root(&mut self) -> Result<()> {
        let status = Command::new("sudo")
            .args(["-n", "true"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| Error::Elevation(format!("não consegui executar sudo: {e}")))?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::Elevation(
                "precisa de root e o sudo exige senha (modo não interativo)".to_string(),
            ))
        }
    }
}
