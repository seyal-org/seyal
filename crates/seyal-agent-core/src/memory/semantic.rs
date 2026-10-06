//! Semantic-key and applicability canonicalization (SPEC-012 §3.2 / §3.3).
//!
//! Free-form profile v1 collapses profile whitespace and LF-normalizes line
//! endings in this pure-domain crate. Full Unicode NFC is applied by
//! `seyal-agent-store` before durable commit so `seyal-agent-core` stays
//! dependency-free.

use super::caps::SEMANTIC_KEY_PROFILE_V1;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CanonicalBytes(pub Vec<u8>);

impl CanonicalBytes {
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Free-form semantic-key profile v1 whitespace/LF stage (SPEC-012 §3.2).
pub fn normalize_freeform_v1(input: &str) -> CanonicalBytes {
    let mut out = String::with_capacity(input.len());
    let mut prev_space = false;
    for ch in input.chars() {
        let c = if ch == '\r' {
            '\n'
        } else if is_profile_whitespace(ch) {
            ' '
        } else {
            ch
        };
        if c == ' ' {
            if prev_space || out.is_empty() {
                continue;
            }
            prev_space = true;
            out.push(' ');
        } else if c == '\n' {
            while out.ends_with(' ') {
                out.pop();
            }
            if out.ends_with('\n') {
                continue;
            }
            prev_space = false;
            out.push('\n');
        } else {
            prev_space = false;
            out.push(c);
        }
    }
    while out.ends_with(' ') || out.ends_with('\n') {
        out.pop();
    }
    CanonicalBytes(out.into_bytes())
}

fn is_profile_whitespace(ch: char) -> bool {
    matches!(
        ch,
        '\u{0009}' | '\u{000B}' | '\u{000C}' | '\u{0020}' | '\u{00A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
    )
}

/// Structured semantic identity: schema_id/version + sorted field encodings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructuredField {
    pub field_id: u32,
    pub value: StructuredValue,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StructuredValue {
    Absent,
    Null,
    Bool(bool),
    Int(i64),
    Text(String),
    List(Vec<StructuredValue>),
    /// Set semantics: elements are canonicalized then byte-sorted unique.
    Set(Vec<StructuredValue>),
}

pub fn encode_structured_key(
    schema_id: u32,
    schema_version: u16,
    mut fields: Vec<StructuredField>,
) -> CanonicalBytes {
    fields.sort_by_key(|f| f.field_id);
    let mut out = Vec::new();
    write_u32(&mut out, schema_id);
    write_u16(&mut out, schema_version);
    write_u32(&mut out, fields.len() as u32);
    for field in fields {
        write_u32(&mut out, field.field_id);
        encode_value(&mut out, &field.value);
    }
    CanonicalBytes(out)
}

fn encode_value(out: &mut Vec<u8>, value: &StructuredValue) {
    match value {
        StructuredValue::Absent => out.push(0),
        StructuredValue::Null => out.push(1),
        StructuredValue::Bool(v) => {
            out.push(2);
            out.push(u8::from(*v));
        }
        StructuredValue::Int(v) => {
            out.push(3);
            let s = v.to_string();
            write_bytes(out, s.as_bytes());
        }
        StructuredValue::Text(v) => {
            out.push(4);
            let normalized = normalize_freeform_v1(v);
            write_bytes(out, normalized.as_slice());
        }
        StructuredValue::List(items) => {
            out.push(5);
            write_u32(out, items.len() as u32);
            for item in items {
                encode_value(out, item);
            }
        }
        StructuredValue::Set(items) => {
            out.push(6);
            let mut encoded: Vec<Vec<u8>> = items
                .iter()
                .map(|item| {
                    let mut buf = Vec::new();
                    encode_value(&mut buf, item);
                    buf
                })
                .collect();
            encoded.sort();
            encoded.dedup();
            write_u32(out, encoded.len() as u32);
            for item in encoded {
                out.extend_from_slice(&item);
            }
        }
    }
}

fn write_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    write_u32(out, bytes.len() as u32);
    out.extend_from_slice(bytes);
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SemanticIdentity {
    pub version: u16,
    pub canonical: CanonicalBytes,
}

impl SemanticIdentity {
    pub fn freeform_v1(statement: &str) -> Self {
        Self {
            version: SEMANTIC_KEY_PROFILE_V1,
            canonical: normalize_freeform_v1(statement),
        }
    }

    pub fn structured(schema_id: u32, schema_version: u16, fields: Vec<StructuredField>) -> Self {
        Self {
            version: schema_version,
            canonical: encode_structured_key(schema_id, schema_version, fields),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ApplicabilityIdentity {
    pub schema_version: u16,
    pub canonical: CanonicalBytes,
}

impl ApplicabilityIdentity {
    pub fn structured(schema_version: u16, fields: Vec<StructuredField>) -> Self {
        Self {
            schema_version,
            canonical: encode_structured_key(1, schema_version, fields),
        }
    }

    pub fn empty_v1() -> Self {
        Self::structured(super::caps::APPLICABILITY_SCHEMA_V1, Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freeform_v1_collapses_whitespace_and_nfc() {
        let a = normalize_freeform_v1("  hello\t\tworld  ");
        let b = normalize_freeform_v1("hello world");
        assert_eq!(a, b);
    }

    #[test]
    fn structured_field_order_is_canonical() {
        let left = encode_structured_key(
            7,
            1,
            vec![
                StructuredField {
                    field_id: 2,
                    value: StructuredValue::Text("b".into()),
                },
                StructuredField {
                    field_id: 1,
                    value: StructuredValue::Text("a".into()),
                },
            ],
        );
        let right = encode_structured_key(
            7,
            1,
            vec![
                StructuredField {
                    field_id: 1,
                    value: StructuredValue::Text("a".into()),
                },
                StructuredField {
                    field_id: 2,
                    value: StructuredValue::Text("b".into()),
                },
            ],
        );
        assert_eq!(left, right);
    }
}
