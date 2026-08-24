use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

fn profile_document() -> PathBuf {
    PathBuf::from(env::var_os("HOME").expect("HOME is set"))
        .join(".codex")
        .join("AGENTS.md")
}

fn disabled_document() -> PathBuf {
    profile_document().with_file_name("AGENTS.md.no-auto-inject")
}

fn state() -> &'static str {
    if disabled_document().exists() {
        "off"
    } else {
        "on"
    }
}

fn move_document(source: &Path, destination: &Path) {
    if source.exists() {
        fs::rename(source, destination).expect("rename fixture document");
    }
}

fn write_claude_profiles() {
    let root = PathBuf::from(env::var_os("HOME").expect("HOME is set"));
    let mut output = io::stdout().lock();
    for profile in [root.join(".claude"), root.join(".claude_work")] {
        output
            .write_all(profile.as_os_str().as_encoded_bytes())
            .unwrap();
        output.write_all(&[0]).unwrap();
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("status") => println!("{}", state()),
        Some("disable") => move_document(&profile_document(), &disabled_document()),
        Some("enable") => move_document(&disabled_document(), &profile_document()),
        Some("toggle") if state() == "on" => {
            move_document(&profile_document(), &disabled_document())
        }
        Some("toggle") => move_document(&disabled_document(), &profile_document()),
        Some("probe-write") => match args
            .get(1)
            .map(|path| fs::write(path, b"probe"))
            .transpose()
        {
            Ok(Some(())) => println!("wrote outside session"),
            Ok(None) => std::process::exit(2),
            Err(error) => {
                eprintln!("blocked: {error}");
                std::process::exit(23);
            }
        },
        Some("probe-read") => match args.get(1).map(fs::read).transpose() {
            Ok(Some(_)) => println!("read outside session"),
            Ok(None) => std::process::exit(2),
            Err(error) => {
                eprintln!("blocked: {error}");
                std::process::exit(23);
            }
        },
        Some("tray") => loop {
            thread::sleep(Duration::from_secs(60));
        },
        Some("profiles") | Some("__installer-claude-homes") => write_claude_profiles(),
        Some("--version") => println!("agent-instructions 0.0.0"),
        _ => std::process::exit(2),
    }
}
