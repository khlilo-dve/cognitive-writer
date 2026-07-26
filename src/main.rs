mod clipboard;
mod error;
mod generate;
mod intent;
mod io;
mod learn;
mod llm;
mod refine;
mod repl;
mod styles;
mod update;
mod website;

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
    let rt = tokio::runtime::Runtime::new().expect("无法创建 tokio runtime");
    rt.block_on(async {
        let mut repl = match crate::repl::Repl::new() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[error] 初始化失败: {}", e);
                std::process::exit(1);
            }
        };
        repl.run().await;
    });
}

fn run_cli_mode(args: Vec<String>) {
    let subcommand = args.get(1).map(|s| s.as_str());
    let remaining: Vec<&str> = args.iter().skip(2).map(|s| s.as_str()).collect();

    let rt = tokio::runtime::Runtime::new().expect("无法创建 tokio runtime");
    rt.block_on(async {
        match subcommand {
            Some("generate") => {
                let input = extract_arg(&remaining, "-i", "--input")
                    .unwrap_or("inputs/idea_01.md");
                let no_clipboard = remaining.contains(&"--no-clipboard");
                crate::generate::run_generate(input, no_clipboard).await;
            }
            Some("learn") => {
                if let Some(url) = remaining.first().copied() {
                    crate::learn::run_learn(url).await;
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
                let instruction = extract_arg(&remaining, "-i", "--instruction")
                    .map(|s| s.to_string());
                if let Some(file) = remaining.iter().find(|&a| a.ends_with(".md")) {
                    crate::update::run_update(file, instruction, no_clipboard).await;
                } else {
                    eprintln!("用法: cog update <file.md> [-i <指令>] [--no-clipboard]");
                    std::process::exit(1);
                }
            }
            _ => {
                let cmd = subcommand.unwrap_or("");
                eprintln!("未知命令: {}", cmd);
                eprintln!("用法: cog [generate|learn|refine|update] [...]");
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
