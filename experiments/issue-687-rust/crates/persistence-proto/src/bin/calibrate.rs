use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use persistence_proto::env as bench_env;
use persistence_proto::redact;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        eprintln!("usage: calibrate <g1|g2|g3|g4|g5|g6|g7|g8|g9|g10|g11|all> [--out DIR] [--fixtures DIR] [--reps N]");
        return ExitCode::from(2);
    };
    if command == "g3-child" {
        let boundary: u8 = args.next().and_then(|v| v.parse().ok()).unwrap_or(0);
        let store = args.next().unwrap_or_default();
        return match persistence_proto::g3::child_main(boundary, std::path::Path::new(&store)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::from(1),
        };
    }
    if command == "g3-fence-child" {
        let store = args.next().unwrap_or_default();
        return match persistence_proto::g3::fence_child(std::path::Path::new(&store)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::from(1),
        };
    }
    let mut out = PathBuf::from("experiments/issue-687-rust/results");
    let mut fixtures = PathBuf::from("tests/fixtures/vt");
    let mut reps = 100_u32;
    let mut rest = vec![command];
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => out = PathBuf::from(args.next().unwrap_or_default()),
            "--fixtures" => fixtures = PathBuf::from(args.next().unwrap_or_default()),
            "--reps" => reps = args.next().and_then(|v| v.parse().ok()).unwrap_or(100),
            other => rest.push(other.to_string()),
        }
    }
    let command = rest[0].as_str();
    fs::create_dir_all(&out).ok();
    let env = bench_env::collect();
    write_json(&out.join("env.json"), &env);
    let run = |name: &str| match name {
        "g1" => write_json(&out.join("g1.json"), &persistence_proto::g1::run()),
        "g2" => write_json(&out.join("g2.json"), &persistence_proto::g2::run(&fixtures)),
        "g3" => write_json(&out.join("g3.json"), &persistence_proto::g3::run(reps, 20)),
        "g4" => write_json(&out.join("g4.json"), &persistence_proto::g4::run()),
        "g5" => write_json(&out.join("g5.json"), &persistence_proto::g5::run()),
        "g6" => write_json(&out.join("g6.json"), &persistence_proto::g6::run()),
        "g7" => write_json(&out.join("g7.json"), &persistence_proto::g7::run()),
        "g8" => write_json(&out.join("g8.json"), &persistence_proto::g8::run()),
        "g9" => write_json(&out.join("g9.json"), &persistence_proto::g9::run()),
        "g10" => write_json(&out.join("g10.json"), &persistence_proto::g10::run()),
        "g11" => write_json(&out.join("g11.json"), &persistence_proto::compat::run()),
        "all" => {
            for name in ["g1", "g2", "g3", "g8", "g11", "g4", "g6", "g7", "g10", "g5", "g9"] {
                eprintln!("=== {name} ===");
                match name {
                    "g1" => write_json(&out.join("g1.json"), &persistence_proto::g1::run()),
                    "g2" => write_json(&out.join("g2.json"), &persistence_proto::g2::run(&fixtures)),
                    "g3" => write_json(&out.join("g3.json"), &persistence_proto::g3::run(reps, 20)),
                    "g4" => write_json(&out.join("g4.json"), &persistence_proto::g4::run()),
                    "g5" => write_json(&out.join("g5.json"), &persistence_proto::g5::run()),
                    "g6" => write_json(&out.join("g6.json"), &persistence_proto::g6::run()),
                    "g7" => write_json(&out.join("g7.json"), &persistence_proto::g7::run()),
                    "g8" => write_json(&out.join("g8.json"), &persistence_proto::g8::run()),
                    "g9" => write_json(&out.join("g9.json"), &persistence_proto::g9::run()),
                    "g10" => write_json(&out.join("g10.json"), &persistence_proto::g10::run()),
                    "g11" => write_json(&out.join("g11.json"), &persistence_proto::compat::run()),
                    _ => {}
                }
            }
        }
        _ => eprintln!("unknown command"),
    };
    run(command);
    ExitCode::SUCCESS
}

fn write_json<T: serde::Serialize>(path: &std::path::Path, value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(text) => {
            let clean = redact::sanitize_log(&text);
            if fs::write(path, clean).is_err() {
                eprintln!("could not write result");
            }
        }
        Err(_) => eprintln!("could not encode result"),
    }
}
