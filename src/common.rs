use crate::error::ToolError;
use immutable_json::serde;
use immutable_json::{api::Value, object::Object};
use std::fs;
use std::fs::{DirBuilder, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;

fn create_directory(path: &Path) -> Result<(), ToolError> {
    Ok(DirBuilder::new().recursive(true).create(path)?)
}

fn create_file(directory: &Path, filename: &str) -> Result<File, ToolError> {
    let path = path(directory, filename);

    if let Some(p) = path.parent() {
        create_directory(p)?;
    }

    Ok(File::create(path)?)
}

pub(crate) fn get_file(directory: &Path, filename: &str) -> Result<File, ToolError> {
    let path = path(directory, filename);

    if !path.exists() {
        create_file(directory, filename)
    } else {
        Ok(OpenOptions::new().truncate(true).read(true).write(true).open(path)?)
    }
}

pub(crate) fn path(directory: &Path, filename: &str) -> PathBuf {
    let mut path = PathBuf::from(directory);

    path.push(filename);
    path
}

pub(crate) fn read_json(path: &Path) -> Result<Object, ToolError> {
    Value::from_str(&fs::read_to_string(path)?)?
        .as_object()
        .ok_or(ToolError::JsonObject)
}

pub(crate) fn read_json_or_yaml(path: &Path) -> Result<Object, ToolError> {
    if path.ends_with(".json") {
        read_json(path)
    } else if path.extension().is_some_and(|ext| ext == "yaml" || ext == "yml") {
        read_yaml(path)
    } else {
        Err(ToolError::JsonOrYaml)
    }
}

pub(crate) fn read_yaml(path: &Path) -> Result<Object, ToolError> {
    let yaml: serde_json::Value = serde_yaml::from_str(&fs::read_to_string(path)?)?;

    serde::from_value(&yaml)
        .and_then(|v| v.as_object())
        .ok_or(ToolError::JsonObject)
}

pub(crate) fn to_yaml(value: &Value) -> Result<String, ToolError> {
    Ok(serde_yaml::to_string(&serde::to_value(value).unwrap())?)
}

pub(crate) fn write_string(s: &str, file: &mut File) -> Result<(), ToolError> {
    file.write_all(s.as_bytes())?;
    Ok(())
}

pub(crate) fn write_yaml(value: &Value, file: &mut File) -> Result<(), ToolError> {
    write_string(&to_yaml(value)?, file)
}
