//! Letras y Líneas game logic: lyric storage, AI grading, and review scheduling.
//!
//! Nothing here depends on Tauri, so the same crate can sit behind a web server later.

pub mod ai;
pub mod claude;
pub mod codex;
pub mod db;
pub mod lookup;
pub mod lyrics;
pub mod models;
pub mod proofread;
pub mod provider;
pub mod review;
pub mod service;
pub mod settings;
pub mod spoilers;
pub mod transcribe;
pub mod tutor;
pub mod users;

pub use ai::Ai;
pub use claude::Claude;
pub use db::Db;
pub use users::{User, Users};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("AI provider error ({status}): {message}")]
    Api { status: u16, message: String },
    #[error("The AI declined to answer this one")]
    Refusal,
    #[error("The AI's reply was cut off; try again")]
    Truncated,
    #[error("The AI's grade came back without its feedback; try Regrade")]
    Incomplete,
    #[error("unexpected reply from the AI: {0}")]
    BadResponse(String),
    #[error("{0}")]
    Unavailable(String),
    #[error("{0} not found")]
    NotFound(&'static str),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;
