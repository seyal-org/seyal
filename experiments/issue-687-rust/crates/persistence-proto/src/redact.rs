//! Synthetic canary handling. The token is a calibration marker, not a credential.
//! Evidence and logs must not reproduce it.

pub const CANARY: &str = "SEYAL-CANARY-687-SYNTHETIC-9f3c1a7e";

pub fn contains_canary(bytes: &[u8]) -> bool {
    bytes.windows(CANARY.len()).any(|window| window == CANARY.as_bytes())
}

pub fn sanitize_log(input: &str) -> String {
    let mut out = input.replace(CANARY, "[redacted]");
    out = strip_path_prefix(&out, "/Users/");
    out = strip_path_prefix(&out, "/private/var/");
    out = strip_path_prefix(&out, "/var/folders/");
    out = strip_path_prefix(&out, "/tmp/");
    out = strip_path_prefix(&out, "/Volumes/");
    out
}

fn strip_path_prefix(input: &str, prefix: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(index) = rest.find(prefix) {
        out.push_str(&rest[..index]);
        out.push_str("[path]");
        let after = &rest[index + prefix.len()..];
        let skip = after.find(char::is_whitespace).unwrap_or(after.len());
        rest = &after[skip..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_line_drops_canary_and_home_path() {
        let line = format!("open {CANARY} at /Users/example/Library/Application Support/Seyal");
        let clean = sanitize_log(&line);
        assert!(!clean.contains(CANARY));
        assert!(!clean.contains("/Users/"));
        assert!(clean.contains("[redacted]"));
        assert!(clean.contains("[path]"));
    }
}
