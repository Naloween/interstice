use std::{
    fmt::Display,
    fs::File,
    io::{self, Write},
    sync::{Arc, Mutex},
};

use colored_text::Colorize;

#[derive(Clone)]
pub struct Logger {
    log_sink: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl Logger {
    pub fn new(log_file: File) -> Self {
        Self {
            log_sink: Arc::new(Mutex::new(Box::new(log_file))),
        }
    }

    pub fn new_stdout() -> Self {
        Self {
            log_sink: Arc::new(Mutex::new(Box::new(io::sink()))),
        }
    }

    pub fn log(&self, message: &str, source: LogSource, level: LogLevel) {
        let mut file = self.log_sink.lock().unwrap();
        let message = format!("[{}] [{}] {}", source, level, message);
        writeln!(file, "{}", message).expect("Should be able to write to log file");
        let mut stdout = std::io::stdout();
        let _ = writeln!(stdout, "{}", message);
    }
}

pub enum LogSource {
    Node,
    Network,
    App,
    Runtime,
    Module(String),
}

impl Display for LogSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogSource::Node => write!(f, "{}", "Node".yellow()),
            LogSource::Network => write!(f, "{}", "Network".yellow()),
            LogSource::App => write!(f, "{}", "App".yellow()),
            LogSource::Runtime => write!(f, "{}", "Runtime".yellow()),
            LogSource::Module(name) => write!(f, "{}", name.blue()),
        }
    }
}

pub enum LogLevel {
    Info,
    Warning,
    Error,
}

impl Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Info => write!(f, "{}", "INFO".green()),
            LogLevel::Warning => write!(f, "{}", "WARNING".yellow()),
            LogLevel::Error => write!(f, "{}", "ERROR".red()),
        }
    }
}
