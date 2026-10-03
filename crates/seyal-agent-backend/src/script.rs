//! Fixture-host script parser (AB-1.9).

use crate::HostObservationKind;

pub fn parse_script(text: &str) -> Result<Vec<crate::ScriptStep>, crate::ScriptError> {
    if text.len() > 64 * 1024 || text.lines().count() > 1024 {
        return Err(crate::ScriptError::ScriptTooLarge);
    }
    let mut steps = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        steps.push(parse_line(line)?);
    }
    if steps.is_empty() {
        return Err(crate::ScriptError::EmptyScript);
    }
    Ok(steps)
}

fn parse_line(line: &str) -> Result<crate::ScriptStep, crate::ScriptError> {
    let mut parts = line.split_whitespace();
    let verb = parts.next().ok_or(crate::ScriptError::MalformedScript)?;
    match verb {
        "duplicate" => Ok(crate::ScriptStep::DuplicateLast),
        "delay" => {
            let ticks = parts
                .next()
                .ok_or(crate::ScriptError::MalformedScript)?
                .parse()
                .map_err(|_| crate::ScriptError::MalformedScript)?;
            Ok(crate::ScriptStep::DelayTicks(ticks))
        }
        "emit" => {
            let kind = parts.next().ok_or(crate::ScriptError::MalformedScript)?;
            let kind = match kind {
                "started" => HostObservationKind::Started,
                "progress" => {
                    let step = parts
                        .next()
                        .ok_or(crate::ScriptError::MalformedScript)?
                        .parse()
                        .map_err(|_| crate::ScriptError::MalformedScript)?;
                    HostObservationKind::Progress { step }
                }
                "disconnect" => HostObservationKind::ObservationDisconnected,
                "reconnect" => HostObservationKind::ObservationReconnected,
                "success" => HostObservationKind::KnownSuccess,
                "failure" => HostObservationKind::KnownFailure,
                "crash" => HostObservationKind::HarnessCrashed,
                "unknown-liveness" => HostObservationKind::UnknownLiveness,
                "effect-unknown" => HostObservationKind::EffectUnknown,
                "result" | "output" => {
                    let hex = parts.next().ok_or(crate::ScriptError::MalformedScript)?;
                    let bytes = decode_hex(hex)?;
                    if kind == "result" {
                        HostObservationKind::Result(bytes)
                    } else {
                        HostObservationKind::Output(bytes)
                    }
                }
                _ => return Err(crate::ScriptError::MalformedScript),
            };
            if parts.next().is_some() {
                return Err(crate::ScriptError::MalformedScript);
            }
            Ok(crate::ScriptStep::Emit(kind))
        }
        _ => Err(crate::ScriptError::MalformedScript),
    }
}

fn decode_hex(text: &str) -> Result<Vec<u8>, crate::ScriptError> {
    // Byte length can be even while a window still splits a multibyte scalar.
    // Reject that before slicing so malformed harness text stays an error.
    if !text.is_ascii() || !text.len().is_multiple_of(2) || text.len() > 8192 {
        return Err(crate::ScriptError::MalformedScript);
    }
    (0..text.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&text[index..index + 2], 16)
                .map_err(|_| crate::ScriptError::MalformedScript)
        })
        .collect()
}
