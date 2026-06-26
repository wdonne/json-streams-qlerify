use std::{io, str::Utf8Error};
use thiserror::Error;

#[derive(Error, Debug)]
pub(crate) enum ToolError {
    #[error("field with no name")]
    FieldNoName,
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("invalid JSON pointer {0}")]
    InvalidPointer(String),
    #[error("{0}")]
    Json(#[from] immutable_json::error::Error),
    #[error("must be a JSON object")]
    JsonObject,
    #[error("must be a JSON or a YAML file")]
    JsonOrYaml,
    #[error("no bounded context")]
    NoBoundedContext,
    #[error("no command for event {0}")]
    NoCommand(String),
    #[error("no domain events")]
    NoDomainEvents,
    #[error("no schema: {0}")]
    NoSchema(String),
    #[error("UTF-8 encoding error: {0}")]
    Utf8(#[from] Utf8Error),
    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),
}
