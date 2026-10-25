//! The JSON output shell.

use std::io;

use serde::Serialize;

use super::Ui;

/// The version of the JSON output schema.
pub const SCHEMA_VERSION: u32 = 1;

/// The document shell shared by all commands.
#[derive(Serialize)]
struct Document<'a, T> {
    schema: u32,
    command: &'a str,
    #[serde(flatten)]
    body: T,
}

/// Writes a JSON document to stdout when the JSON format is selected.
pub fn write<T: Serialize>(ui: &mut Ui, command: &str, body: &T) -> io::Result<()> {
    ui.json(&Document {
        schema: SCHEMA_VERSION,
        command,
        body,
    })
}
