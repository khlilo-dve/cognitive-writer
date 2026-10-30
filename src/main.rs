use cognitive_writer::{generate, learn, refine, repl, update};

fn main() {
    if let Err(e) = dotenvy::dotenv() {
        eprintln!("[warn] 未加载 .env: {} (将使用系统环境变量)", e);
    }

    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        run_cli_mode(args);
    } else {
        run_repl_mode();
    }
}

fn run_repl_mode() {
    let session_dir = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".cognitive-writer");
    let rt = tokio::runtime::Runtime::new().expect("无法创建 tokio runtime");
    if let Err(e) = rt.block_on(repl::run_session(&session_dir)) {
        eprintln!("[error] {e}");
        std::process::exit(1);
    }
}

fn run_cli_mode(args: Vec<String>) {
    let subcommand = args.get(1).map(|s| s.as_str());
    let remaining: Vec<&str> = args.iter().skip(2).map(|s| s.as_str()).collect();

    let rt = tokio::runtime::Runtime::new().expect("无法创建 tokio runtime");
    rt.block_on(async {
        match subcommand {
            Some("style-draft") => {
                if let Err(e) = cognitive_writer::learning_workspace_cli::run(&remaining) {
                    eprintln!("[error] {e}");
                    std::process::exit(1);
                }
            }
            Some("research") => {
                if let Err(e) = cognitive_writer::research_run::run(&remaining).await {
                    eprintln!("[error] {e}");
                    std::process::exit(1);
                }
            }
            Some("generate") => {
                let input = extract_arg(&remaining, "-i", "--input").unwrap_or("inputs/idea_01.md");
                let no_clipboard = remaining.contains(&"--no-clipboard");
                crate::generate::run_generate(input, no_clipboard).await;
            }
            Some("learn") => {
                if let Some(url) = remaining.first().copied() {
                    if let Err(e) = crate::learn::run_learn(url).await {
                        eprintln!("[error] {}", e);
                        std::process::exit(1);
                    }
                } else {
                    eprintln!("用法: cog learn <URL>");
                    std::process::exit(1);
                }
            }
            Some("refine") => {
                let no_clipboard = remaining.contains(&"--no-clipboard");
                if let Some(file) = remaining.iter().find(|&a| a.ends_with(".md")) {
                    crate::refine::run_refine(file, no_clipboard).await;
                } else {
                    eprintln!("用法: cog refine <file.md> [--no-clipboard]");
                    std::process::exit(1);
                }
            }
            Some("update") => {
                let no_clipboard = remaining.contains(&"--no-clipboard");
                let instruction =
                    extract_arg(&remaining, "-i", "--instruction").map(|s| s.to_string());
                if let Some(file) = remaining.iter().find(|&a| a.ends_with(".md")) {
                    if let Err(e) = crate::update::run_update(file, instruction, no_clipboard).await
                    {
                        eprintln!("[error] {}", e);
                        std::process::exit(1);
                    }
                } else {
                    eprintln!("用法: cog update <file.md> [-i <指令>] [--no-clipboard]");
                    std::process::exit(1);
                }
            }
            _ => {
                let cmd = subcommand.unwrap_or("");
                eprintln!("未知命令: {}", cmd);
                eprintln!("用法: cog [generate|learn|style-draft|refine|update|research] [...]");
                std::process::exit(1);
            }
        }
    });
}

/// 提取命令行标志后面的值（支持短标志和长标志）。
fn extract_arg<'a>(args: &[&'a str], short: &str, long: &str) -> Option<&'a str> {
    for i in 0..args.len() {
        if args[i] == short || args[i] == long {
            return args.get(i + 1).copied();
        }
    }
    None
}
