//! Eventos emitidos pelo motor. CLI, TUI e GUI só consomem isto.

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Started { module: String, what: String },
    Progress { module: String, done: u64, total: Option<u64> },
    Log { module: String, line: String },
    Skipped { module: String, reason: String },
    /// Config mantida como está; a versão nova ficou pendente de resolução.
    Conflict { module: String, path: String },
    Finished { module: String },
}

impl Event {
    /// Texto curto de uma linha, para logs.
    pub fn describe(&self) -> String {
        match self {
            Event::Started { module, what } => format!("{module}: {what}"),
            Event::Progress { module, done, total } => match total {
                Some(t) => format!("{module}: {done}/{t} bytes"),
                None => format!("{module}: {done} bytes"),
            },
            Event::Log { module, line } => format!("{module}: {line}"),
            Event::Skipped { module, reason } => format!("{module}: pulado ({reason})"),
            Event::Conflict { module, path } => format!("{module}: CONFLITO em {path}"),
            Event::Finished { module } => format!("{module}: concluído"),
        }
    }
}

pub trait EventSink {
    fn emit(&mut self, event: Event);
}

impl<F: FnMut(Event)> EventSink for F {
    fn emit(&mut self, event: Event) {
        self(event)
    }
}

/// Descarta tudo.
pub struct NullSink;

impl EventSink for NullSink {
    fn emit(&mut self, _event: Event) {}
}

/// Acumula eventos (útil em testes).
#[derive(Default)]
pub struct VecSink(pub Vec<Event>);

impl EventSink for VecSink {
    fn emit(&mut self, event: Event) {
        self.0.push(event);
    }
}
