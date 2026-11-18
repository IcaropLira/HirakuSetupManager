//! hiraku-core: a base compartilhada por CLI, TUI e GUI.
//!
//! Regra de ouro: este crate não imprime nem pergunta nada ao usuário.
//! Quem interage é o frontend; o core emite `Event`s e pede privilégio via `Elevator`.

pub mod bump;
pub mod configs;
pub mod configstate;
pub mod elevate;
pub mod env;
pub mod error;
pub mod event;
pub mod exec;
pub mod hooks;
pub mod installer;
pub mod lockfile;
pub mod manifest;
pub mod paths;
pub mod pkg;
pub mod regsource;
pub mod registry;
pub mod release;
pub mod schedule;
pub mod settings;
pub mod sync;
pub mod verify;

pub use configstate::ConfigState;
pub use elevate::{Elevator, NoElevation, NonInteractiveSudo, SudoElevator};
pub use env::{Arch, Environment, Os, PkgManager, Privilege};
pub use error::{Error, Result};
pub use event::{Event, EventSink};
pub use lockfile::{LockedModule, Lockfile};
pub use manifest::Manifest;
