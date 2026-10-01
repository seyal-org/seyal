//! Cold TOML subset used by Seyal UI config. Not invoked from terminal hot paths.

use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum TomlValue {
    String(String),
    Bool(bool),
    Number(f64),
    Array(Vec<TomlValue>),
    Table(BTreeMap<String, TomlValue>),
}

impl TomlValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_string_array(&self) -> Option<Vec<String>> {
        match self {
            Self::Array(values) => {
                let mut strings = Vec::with_capacity(values.len());
                for value in values {
                    strings.push(value.as_str()?.to_owned());
                }
                Some(strings)
            }
            _ => None,
        }
    }

    pub fn as_table(&self) -> Option<&BTreeMap<String, TomlValue>> {
        match self {
            Self::Table(table) => Some(table),
            _ => None,
        }
    }

    /// Array-of-tables (`[[name]]`) values: every element must be a table.
    pub fn as_array_of_tables(&self) -> Option<Vec<&BTreeMap<String, TomlValue>>> {
        match self {
            Self::Array(values) => {
                let mut tables = Vec::with_capacity(values.len());
                for value in values {
                    tables.push(value.as_table()?);
                }
                Some(tables)
            }
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TomlError(pub String);

#[derive(Clone, Debug)]
enum WriteTarget {
    /// Nested table path such as `[ui]` / `[input]`.
    Table(Vec<String>),
    /// Last element of an array-of-tables such as `[[keybindings]]`.
    ArrayTable { key: String, index: usize },
}

pub fn parse_toml(text: &str) -> Result<BTreeMap<String, TomlValue>, TomlError> {
    let mut root = BTreeMap::new();
    let mut target = WriteTarget::Table(Vec::new());
    for (index, raw_line) in text.split('\n').enumerate() {
        let line_number = index + 1;
        let stripped = strip_comment(raw_line).trim().to_owned();
        if stripped.is_empty() {
            continue;
        }
        if stripped.starts_with("[[") {
            if !stripped.ends_with("]]") {
                return Err(TomlError(format!(
                    "line {line_number}: invalid array-of-tables header"
                )));
            }
            let name = stripped[2..stripped.len() - 2].trim();
            if name.is_empty() || name.contains('.') {
                return Err(TomlError(format!(
                    "line {line_number}: unsupported array-of-tables name"
                )));
            }
            let index = push_array_table(&mut root, name);
            target = WriteTarget::ArrayTable {
                key: name.to_owned(),
                index,
            };
            continue;
        }
        if stripped.starts_with('[') {
            if !stripped.ends_with(']') {
                return Err(TomlError(format!(
                    "line {line_number}: invalid table header"
                )));
            }
            let name = stripped[1..stripped.len() - 1].trim();
            if name.is_empty() {
                return Err(TomlError(format!("line {line_number}: empty table name")));
            }
            target = WriteTarget::Table(name.split('.').map(str::to_owned).collect());
            continue;
        }
        let Some(equals) = stripped.find('=') else {
            return Err(TomlError(format!(
                "line {line_number}: expected key = value"
            )));
        };
        let key = stripped[..equals].trim();
        let raw_value = stripped[equals + 1..].trim();
        if key.is_empty() {
            return Err(TomlError(format!("line {line_number}: empty key")));
        }
        let value = parse_value(raw_value)
            .map_err(|error| TomlError(format!("line {line_number}: {}", error.0)))?;
        match &target {
            WriteTarget::Table(path) => {
                let mut full = path.clone();
                full.push(key.to_owned());
                set_path(&mut root, &full, value);
            }
            WriteTarget::ArrayTable {
                key: array_key,
                index,
            } => {
                set_array_table_field(&mut root, array_key, *index, key, value)?;
            }
        }
    }
    Ok(root)
}

fn push_array_table(root: &mut BTreeMap<String, TomlValue>, name: &str) -> usize {
    match root.get_mut(name) {
        Some(TomlValue::Array(items)) => {
            items.push(TomlValue::Table(BTreeMap::new()));
            items.len() - 1
        }
        _ => {
            root.insert(
                name.to_owned(),
                TomlValue::Array(vec![TomlValue::Table(BTreeMap::new())]),
            );
            0
        }
    }
}

fn set_array_table_field(
    root: &mut BTreeMap<String, TomlValue>,
    array_key: &str,
    index: usize,
    field: &str,
    value: TomlValue,
) -> Result<(), TomlError> {
    let Some(TomlValue::Array(items)) = root.get_mut(array_key) else {
        return Err(TomlError(format!(
            "array-of-tables '{array_key}' missing while writing field"
        )));
    };
    let Some(TomlValue::Table(table)) = items.get_mut(index) else {
        return Err(TomlError(format!(
            "array-of-tables '{array_key}' index {index} missing while writing field"
        )));
    };
    table.insert(field.to_owned(), value);
    Ok(())
}

fn set_path(root: &mut BTreeMap<String, TomlValue>, path: &[String], value: TomlValue) {
    fn write(table: &mut BTreeMap<String, TomlValue>, remaining: &[String], value: TomlValue) {
        let Some((head, rest)) = remaining.split_first() else {
            return;
        };
        if rest.is_empty() {
            table.insert(head.clone(), value);
            return;
        }
        let mut child = match table.get(head) {
            Some(TomlValue::Table(existing)) => existing.clone(),
            _ => BTreeMap::new(),
        };
        write(&mut child, rest, value);
        table.insert(head.clone(), TomlValue::Table(child));
    }
    write(root, path, value);
}

fn strip_comment(line: &str) -> String {
    let mut in_string = false;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && in_string {
            escaped = true;
            continue;
        }
        if character == '"' {
            in_string = !in_string;
            continue;
        }
        if character == '#' && !in_string {
            return line[..index].to_owned();
        }
    }
    line.to_owned()
}

fn parse_value(raw: &str) -> Result<TomlValue, TomlError> {
    if raw == "true" {
        return Ok(TomlValue::Bool(true));
    }
    if raw == "false" {
        return Ok(TomlValue::Bool(false));
    }
    if raw.starts_with('"') {
        return parse_string(raw).map(TomlValue::String);
    }
    if raw.starts_with('\'') {
        return parse_literal_string(raw).map(TomlValue::String);
    }
    if raw.starts_with('[') {
        return parse_array(raw);
    }
    if let Ok(number) = raw.parse::<f64>() {
        return Ok(TomlValue::Number(number));
    }
    Err(TomlError(format!("unsupported value '{raw}'")))
}

/// TOML single-quoted literal string (no escapes; used for keys like `cmd+\``).
fn parse_literal_string(raw: &str) -> Result<String, TomlError> {
    if raw.len() < 2 || !raw.starts_with('\'') || !raw.ends_with('\'') {
        return Err(TomlError("unterminated literal string".into()));
    }
    Ok(raw[1..raw.len() - 1].to_owned())
}

fn parse_string(raw: &str) -> Result<String, TomlError> {
    if raw.len() < 2 || !raw.starts_with('"') || !raw.ends_with('"') {
        return Err(TomlError("unterminated string".into()));
    }
    let inner = &raw[1..raw.len() - 1];
    let mut result = String::new();
    let mut escaped = false;
    for character in inner.chars() {
        if escaped {
            result.push(match character {
                'n' => '\n',
                't' => '\t',
                '"' => '"',
                '\\' => '\\',
                other => other,
            });
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            result.push(character);
        }
    }
    if escaped {
        return Err(TomlError("unterminated escape".into()));
    }
    Ok(result)
}

fn parse_array(raw: &str) -> Result<TomlValue, TomlError> {
    if !raw.starts_with('[') || !raw.ends_with(']') {
        return Err(TomlError("unterminated array".into()));
    }
    let inner = raw[1..raw.len() - 1].trim();
    if inner.is_empty() {
        return Ok(TomlValue::Array(Vec::new()));
    }
    let mut items = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut escaped = false;
    for character in inner.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' && in_string {
            current.push(character);
            escaped = true;
            continue;
        }
        if character == '"' {
            in_string = !in_string;
            current.push(character);
            continue;
        }
        if character == ',' && !in_string {
            items.push(parse_value(current.trim())?);
            current.clear();
            continue;
        }
        current.push(character);
    }
    if in_string {
        return Err(TomlError("unterminated string in array".into()));
    }
    if !current.trim().is_empty() {
        items.push(parse_value(current.trim())?);
    }
    Ok(TomlValue::Array(items))
}
