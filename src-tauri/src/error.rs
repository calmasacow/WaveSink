use thiserror::Error;

/// All backend errors. Tauri commands convert these to `String` for
/// serialisation across IPC (`Result<T, String>`).
#[derive(Debug, Error)]
pub enum SinkError {
    #[error("could not parse: {0}")]
    Parse(String),

    #[error("unknown virtual sink: {0}")]
    UnknownSink(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
