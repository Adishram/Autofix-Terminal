use clap::Parser;
use console::style;

use regex::Regex;
use serde::Deserialize;
use similar::{ChangeTag, TextDiff};
use std::fs;
use std::io::{self, BufRead, BufReader, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

// ─── CLI Definition ─────────────────────────────────────────────────────────────

/// autofix — an auto-fixing developer tool.
///
/// Run any command; if it fails, an AI will attempt to fix the broken source code.
#[derive(Parser, Debug)]
#[command(name = "autofix", version, about, trailing_var_arg = true)]
struct Cli {
    /// The command (and its arguments) to execute.
    /// Usage: autofix -- <command> [args...]
    #[arg(required = true, num_args = 1..)]
    command: Vec<String>,
}

// ─── Data Structures ────────────────────────────────────────────────────────────

/// Captures information about a failed process execution.
#[derive(Debug)]
struct FailureInfo {
    exit_code: i32,
    stderr: String,
}

/// The JSON payload returned by ai_brain.py.
#[derive(Debug, Deserialize)]
struct AiFix {
    fixed_code: String,
    explanation: String,
}

#[derive(Debug, PartialEq)]
enum ErrorCategory {
    Syntax,
    Fatal,
    Runtime,
}

fn categorize_error(stderr: &str) -> ErrorCategory {
    let lower_stderr = stderr.to_lowercase();
    if lower_stderr.contains("syntaxerror") 
        || lower_stderr.contains("indentationerror") 
        || lower_stderr.contains("taberror")
        || lower_stderr.contains("error: expected") {
        ErrorCategory::Syntax
    } else if lower_stderr.contains("recursionerror")
        || lower_stderr.contains("memoryerror")
        || lower_stderr.contains("assertionerror")
        || lower_stderr.contains("segmentation fault") {
        ErrorCategory::Fatal
    } else {
        ErrorCategory::Runtime
    }
}

// ─── Process Execution ─────────────────────────────────────────────────────────

/// Execute a command, streaming stdout in real-time and capturing stderr.
/// Returns Ok(()) on success (exit 0), or Err(FailureInfo) on non-zero exit.
fn execute_command(cmd: &[String], piped_stdin: Option<&[u8]>) -> Result<(), FailureInfo> {
    let program = &cmd[0];
    let args = &cmd[1..];

    let mut command = Command::new(program);
    command.args(args)
        .stdout(Stdio::inherit()) // stream stdout in real-time
        .stderr(Stdio::piped());   // capture stderr

    if piped_stdin.is_some() {
        command.stdin(Stdio::piped());
    }

    let mut child = command
        .spawn()
        .unwrap_or_else(|e| {
            eprintln!(
                "{} Failed to start command '{}': {}",
                style("✘").red().bold(),
                program,
                e
            );
            std::process::exit(1);
        });

    if let Some(input_data) = piped_stdin {
        if let Some(mut child_stdin) = child.stdin.take() {
            let data = input_data.to_vec();
            std::thread::spawn(move || {
                let _ = child_stdin.write_all(&data);
            });
        }
    }

    // Read stderr in a background-friendly way
    let stderr_handle = child.stderr.take();
    let mut stderr_output = String::new();

    if let Some(stderr) = stderr_handle {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    eprintln!("{}", l); // stream stderr to terminal too
                    stderr_output.push_str(&l);
                    stderr_output.push('\n');
                }
                Err(e) => {
                    eprintln!("Error reading stderr: {}", e);
                }
            }
        }
    }

    let status = child.wait().unwrap_or_else(|e| {
        eprintln!("{} Failed to wait on child process: {}", style("✘").red().bold(), e);
        std::process::exit(1);
    });

    let exit_code = status.code().unwrap_or(-1);

    if exit_code == 0 {
        Ok(())
    } else {
        Err(FailureInfo {
            exit_code,
            stderr: stderr_output,
        })
    }
}

// ─── Source File Detection ──────────────────────────────────────────────────────

/// Attempt to locate the source file that triggered the error by parsing stderr.
/// Supports common error formats from Python, Rust, C/C++, Go, TypeScript/JS, Java.
fn detect_source_file(stderr: &str) -> Option<(PathBuf, Option<usize>)> {
    // Order matters: try the most specific patterns first.
    let patterns: Vec<Regex> = vec![
        // Python: File "path.py", line N
        Regex::new(r#"File "([^"]+\.[a-zA-Z]+)", line (\d+)"#).unwrap(),
        // Rust: --> path.rs:N:N
        Regex::new(r#"-->\s+([^\s:]+\.[a-zA-Z]+):(\d+):\d+"#).unwrap(),
        // C/C++/Go/TS/JS/Java: path.ext:N:N: error/warning
        Regex::new(r#"^([^\s:]+\.[a-zA-Z]+):(\d+):\d*:?\s*(?:error|warning|Error|TypeError|SyntaxError)"#).unwrap(),
        // Generic: path.ext:N
        Regex::new(r#"^([^\s:]+\.[a-zA-Z]+):(\d+)"#).unwrap(),
    ];

    for pattern in &patterns {
        for cap in pattern.captures_iter(stderr) {
            let file_path = PathBuf::from(&cap[1]);
            if file_path.exists() {
                let line_num = cap.get(2).and_then(|m| m.as_str().parse::<usize>().ok());
                return Some((file_path, line_num));
            }
        }
    }

    // Fallback: try to find file paths mentioned anywhere in stderr
    let fallback = Regex::new(r#"([/\w.\-]+\.[a-zA-Z]{1,10})"#).unwrap();
    for cap in fallback.captures_iter(stderr) {
        let candidate = PathBuf::from(&cap[1]);
        if candidate.exists() && candidate.is_file() {
            // Skip common false positives
            let name = candidate.file_name().unwrap_or_default().to_string_lossy();
            if !name.starts_with('.') && !name.contains("lib") {
                return Some((candidate, None));
            }
        }
    }

    None
}

// ─── AI Fixer Invocation ────────────────────────────────────────────────────────

/// Call the Python AI brain script and parse its JSON response.
fn call_ai_brain(file_path: &Path, stderr: &str, line_number: Option<usize>) -> Result<AiFix, String> {
    // Locate ai_brain.py relative to the current executable, falling back to cwd
    let brain_script = find_ai_brain_script();

    let mut command = Command::new("python3");
    command.arg(&brain_script)
           .arg(file_path.to_string_lossy().as_ref())
           .arg(stderr);
           
    if let Some(ln) = line_number {
        command.arg(ln.to_string());
    }

    let output = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to execute ai_brain.py: {}", e))?;

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let stderr_str = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() && stdout_str.trim().is_empty() {
        return Err(format!(
            "ai_brain.py exited with code {}: {}",
            output.status.code().unwrap_or(-1),
            stderr_str
        ));
    }

    let fix: AiFix = serde_json::from_str(stdout_str.trim())
        .map_err(|e| format!("Failed to parse AI response: {} — raw: {}", e, stdout_str))?;

    if fix.fixed_code.is_empty() {
        return Err(format!("AI returned empty fix: {}", fix.explanation));
    }

    Ok(fix)
}

/// Find the ai_brain.py script. Checks next to the executable first, then cwd.
fn find_ai_brain_script() -> PathBuf {
    // Check next to the running binary
    if let Ok(exe) = std::env::current_exe() {
        let alongside = exe.parent().unwrap_or(Path::new(".")).join("ai_brain.py");
        if alongside.exists() {
            return alongside;
        }
    }

    // Check current working directory
    let cwd = PathBuf::from("ai_brain.py");
    if cwd.exists() {
        return cwd;
    }

    // Check the cargo manifest dir (for development)
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ai_brain.py");
    if manifest_dir.exists() {
        return manifest_dir;
    }

    eprintln!(
        "{} Could not locate ai_brain.py. Ensure it is in the same directory as the autofix binary or in the current working directory.",
        style("✘").red().bold()
    );
    std::process::exit(1);
}

// ─── File Backup & Restore ──────────────────────────────────────────────────────

fn backup_file(path: &Path) -> Result<PathBuf, String> {
    let backup_path = path.with_extension(
        format!(
            "{}.bak",
            path.extension()
                .unwrap_or_default()
                .to_string_lossy()
        )
    );
    fs::copy(path, &backup_path)
        .map_err(|e| format!("Failed to create backup of {}: {}", path.display(), e))?;
    Ok(backup_path)
}

fn restore_from_backup(original: &Path, backup: &Path) -> Result<(), String> {
    fs::copy(backup, original)
        .map_err(|e| format!("Failed to restore from backup: {}", e))?;
    fs::remove_file(backup)
        .map_err(|e| format!("Failed to remove backup file: {}", e))?;
    Ok(())
}

fn cleanup_backup(backup: &Path) {
    let _ = fs::remove_file(backup);
}

// ─── Interactive Post-Fix UI ────────────────────────────────────────────────────

fn show_post_fix_ui(explanation: &str, original_path: &Path, backup_path: &Path) {
    println!();
    println!(
        "{}",
        style("✔ Code fixed successfully!").green().bold()
    );

    let term = console::Term::stdout();

    loop {
        println!();
        println!("{}", style("What would you like to do?").bold());
        println!("  {} Show explanation", style("[E]").cyan());
        println!("  {} Show diff", style("[D]").cyan());
        println!("  {} Exit", style("[Q/Enter]").cyan());
        
        let key = match term.read_char() {
            Ok(c) => c.to_ascii_lowercase(),
            Err(_) => break, // Fallback if TTY read fails
        };

        match key {
            'e' => show_explanation(explanation),
            'd' => show_diff(original_path, backup_path),
            'q' | '\n' | '\r' => break,
            _ => {}
        }
    }
}

fn show_explanation(explanation: &str) {
    println!();
    println!("{}", style("─── Explanation ───").cyan().bold());
    println!();
    println!("  {}", explanation);
    println!();
    println!("{}", style("───────────────────").cyan().bold());
}

fn show_diff(fixed_path: &Path, backup_path: &Path) {
    let original = match fs::read_to_string(backup_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Could not read backup file for diff: {}", e);
            return;
        }
    };
    let fixed = match fs::read_to_string(fixed_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Could not read fixed file for diff: {}", e);
            return;
        }
    };

    println!();
    println!("{}", style("─── Diff (original → fixed) ───").cyan().bold());
    println!();

    let diff = TextDiff::from_lines(&original, &fixed);
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Delete => {
                print!("{}", style(format!("- {}", change)).red());
            }
            ChangeTag::Insert => {
                print!("{}", style(format!("+ {}", change)).green());
            }
            ChangeTag::Equal => {
                print!("  {}", change);
            }
        }
    }

    println!();
    println!("{}", style("────────────────────────────────").cyan().bold());
}

// ─── Command Inference ──────────────────────────────────────────────────────────

/// Infers the command to run if the user just passes a filename.
fn resolve_command(args: Vec<String>) -> Vec<String> {
    if args.len() == 1 {
        let path = Path::new(&args[0]);
        if path.exists() && path.is_file() {
            if let Some(ext) = path.extension() {
                let ext_str = ext.to_string_lossy();
                match ext_str.as_ref() {
                    "py" => return vec!["python3".to_string(), args[0].clone()],
                    "js" => return vec!["node".to_string(), args[0].clone()],
                    "ts" => return vec!["npx".to_string(), "tsx".to_string(), args[0].clone()],
                    "sh" => return vec!["bash".to_string(), args[0].clone()],
                    "rs" => return vec!["cargo".to_string(), "run".to_string()],
                    _ => {}
                }
            }
        }
    }
    args
}

// ─── Main ───────────────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();
    let cmd = resolve_command(cli.command);

    let mut piped_stdin_buffer = None;
    if !io::stdin().is_terminal() {
        let mut buffer = Vec::new();
        if io::stdin().read_to_end(&mut buffer).is_ok() {
            piped_stdin_buffer = Some(buffer);
        }
    }

    // Display what we're running
    println!(
        "{} Running: {}",
        style("▶").cyan().bold(),
        style(cmd.join(" ")).bold()
    );
    println!();

    // ── First execution ──────────────────────────────────────────────────
    let failure = match execute_command(&cmd, piped_stdin_buffer.as_deref()) {
        Ok(()) => {
            // Success on first try — nothing to fix
            std::process::exit(0);
        }
        Err(f) => f,
    };

    println!();
    println!(
        "{} {} (exit code {}). {}",
        style("✘").red().bold(),
        style("Failure detected").red().bold(),
        style(failure.exit_code).yellow(),
        style("Prepping for auto-fix...").dim()
    );

    // ── Check LLM connection ─────────────────────────────────────────────
    let brain_script = find_ai_brain_script();
    let status = Command::new("python3")
        .arg(&brain_script)
        .arg("--ping")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    if let Ok(s) = status {
        if !s.success() {
            eprintln!();
            eprintln!("{} Failed to connect to the local LLM endpoint (Ollama).", style("✘").red().bold());
            eprintln!("  Please make sure Ollama is running and a local LLM is available,");
            eprintln!("  or connect to an LLM provider by configuring LITELLM_MODEL and OLLAMA_API_BASE.");
            std::process::exit(1);
        }
    } else {
        eprintln!();
        eprintln!("{} Failed to execute ai_brain.py to check LLM connection.", style("✘").red().bold());
        std::process::exit(1);
    }

    // ── Detect source file ───────────────────────────────────────────────
    let (source_file, error_line) = match detect_source_file(&failure.stderr) {
        Some((p, l)) => (p, l),
        None => {
            eprintln!(
                "{} Could not automatically detect the source file from the error output.",
                style("✘").red().bold()
            );
            eprintln!(
                "  Tip: Make sure the command outputs standard error messages with file paths."
            );
            std::process::exit(1);
        }
    };

    println!(
        "{} Detected source file: {}",
        style("→").cyan().bold(),
        style(source_file.display()).underlined()
    );

    // ── Create backup ────────────────────────────────────────────────────
    let backup_path = match backup_file(&source_file) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{} {}", style("✘").red().bold(), e);
            std::process::exit(1);
        }
    };

    println!(
        "{} Backup created: {}",
        style("→").cyan().bold(),
        style(backup_path.display()).dim()
    );

    let source_file_clone = source_file.clone();
    let backup_path_clone = backup_path.clone();
    ctrlc::set_handler(move || {
        println!("\n{} Ctrl-C received. Restoring original file...", style("⚠").yellow().bold());
        if let Err(e) = restore_from_backup(&source_file_clone, &backup_path_clone) {
            eprintln!("{} Failed to restore backup: {}", style("✘").red().bold(), e);
        } else {
            println!("{} Original file restored from backup.", style("↩").green().bold());
        }
        std::process::exit(130);
    }).expect("Error setting Ctrl-C handler");

    // ── Auto-fix loop ────────────────────────────────────────────────────
    let mut current_stderr = failure.stderr.clone();
    #[allow(unused_assignments)]
    let mut last_explanation = String::from("No explanation available.");

    let mut attempt = 0;

    let mut current_file_content = match fs::read_to_string(&source_file) {
        Ok(c) => c,
        Err(_) => String::new(),
    };

    loop {
        let category = categorize_error(&current_stderr);
        
        if category == ErrorCategory::Fatal {
            println!();
            println!(
                "{} Fatal/Logical error detected. Auto-fix will attempt to fix it anyway...",
                style("⚠").yellow().bold()
            );
        }
        
        attempt += 1;

        println!();
        let attempt_type = if category == ErrorCategory::Syntax { "Syntax Fix Attempt" } else { "Runtime Fix Attempt" };
        println!(
            "{} {} {}...",
            style("⟳").yellow().bold(),
            attempt_type,
            style(attempt).bold()
        );

        // Call the AI brain
        println!(
            "  {} Sending code + error to AI...",
            style("⏳").dim()
        );

        let ai_fix = match call_ai_brain(&source_file, &current_stderr, error_line) {
            Ok(fix) => fix,
            Err(e) => {
                eprintln!(
                    "  {} AI brain error: {}",
                    style("✘").red().bold(),
                    e
                );
                continue;
            }
        };
        
        if ai_fix.fixed_code == current_file_content {
            println!();
            println!(
                "{} AI returned the exact same code. Auto-fix will retry...",
                style("⚠").yellow().bold()
            );
        }
        
        current_file_content = ai_fix.fixed_code.clone();
        last_explanation = ai_fix.explanation.clone();

        println!(
            "  {} AI returned a fix. Applying...",
            style("✔").green()
        );

        // Overwrite the source file with the fix
        if let Err(e) = fs::write(&source_file, &ai_fix.fixed_code) {
            eprintln!(
                "  {} Failed to write fix: {}",
                style("✘").red().bold(),
                e
            );
            continue;
        }

        // Re-run the original command
        println!();
        println!(
            "  {} Re-running: {}",
            style("▶").cyan().bold(),
            style(cmd.join(" ")).bold()
        );
        println!();

        match execute_command(&cmd, piped_stdin_buffer.as_deref()) {
            Ok(()) => {
                // ── Success! Show the interactive UI ─────────────────────
                show_post_fix_ui(&last_explanation, &source_file, &backup_path);
                cleanup_backup(&backup_path);
                std::process::exit(0);
            }
            Err(f) => {
                println!();
                println!(
                    "  {} Still failing (exit code {}).",
                    style("✘").red().bold(),
                    style(f.exit_code).yellow()
                );
                current_stderr = f.stderr;
            }
        }
    }

}
