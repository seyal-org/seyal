// Shared by M002 contract-mode benches. Std-only; included into each bench.
// Do not add crate dependencies here.

fn m002_contract_gate() -> Option<String> {
    std::env::var("SEYAL_M002_CONTRACT_GATE")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn m002_parse_usize_env(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value: &usize| *value > 0)
        .unwrap_or(default)
}

fn m002_write_cohort_file(path: &str, cohort: usize, samples: &[f64]) {
    let mut body = format!("cohort = {cohort}\nsamples = [");
    for (index, value) in samples.iter().enumerate() {
        if index > 0 {
            body.push_str(", ");
        }
        body.push_str(&format!("{value:.9}"));
    }
    body.push_str("]\n");
    std::fs::write(std::path::PathBuf::from(path), body).expect("write M002 cohort file");
}
