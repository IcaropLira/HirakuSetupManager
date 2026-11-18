//! hiraku-hub: a camada de aplicação do hub, sem interface.
//!
//! - `catalog`: módulos e opções (validar, avançar, formatar valores).
//! - `model`: linhas de módulos/conflitos e a foto do estado (`Snapshot`).
//! - `presets`: escolhas do assistente (conjunto recomendado, modos, intervalos).
//! - `services`: ler/gravar settings, conflitos, pré-visualização, agendamento e o
//!   sync numa thread.
//!
//! Quem desenha é a TUI (`hiraku-tui`) ou o app desktop (`hiraku-gui`).

pub mod catalog;
pub mod model;
pub mod presets;
pub mod services;
