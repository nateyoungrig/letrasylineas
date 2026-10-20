//! Letras y Líneas game logic: lyric storage, Claude grading, and review scheduling.
//!
//! Nothing here depends on Tauri, so the same crate can sit behind a web server later.

pub mod claude;
pub mod db;
pub mod lookup;
pub mod lyrics;
pub mod models;
pub mod proofread;
pub mod review;
pub mod service;
pub mod spoilers;
pub mod transcribe;
pub mod tutor;
pub mod users;

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
    #[error("Claude API error ({status}): {message}")]
    Api { status: u16, message: String },
    #[error("Claude declined to answer this one")]
    Refusal,
    #[error("Claude's reply was cut off; try again")]
    Truncated,
    #[error("Claude's grade came back without its feedback; try Regrade")]
    Incomplete,
    #[error("unexpected reply from Claude: {0}")]
    BadResponse(String),
    #[error("{0}")]
    Unavailable(String),
    #[error("{0} not found")]
    NotFound(&'static str),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;
