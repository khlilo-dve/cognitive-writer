//! 旧批量评测 API；新评测优先使用 eval_runs 的检查点。
use crate::error::AppError;
use crate::eval::TEST_SET;
use crate::generate::{generate_outline, render_fulltext};
use reqwest::Client;

/// 跑整批测试集，生成每篇的大纲+正文，存到 target_dir 作为基准或候选文章。
pub async fn eval_style_batch(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    style: &str,
    target_dir: &str,
) -> Result<(), AppError> {
    let target = std::path::Path::new(target_dir);
    if target.exists() && std::fs::read_dir(target)?.next().is_some() {
        return Err(AppError::FileWrite(
            "目标目录非空；使用新目录或可续跑的 eval_automated_suite".into(),
        ));
    }
    std::fs::create_dir_all(target_dir)
        .map_err(|e| AppError::Intent(format!("创建目录失败: {}", e)))?;

    for (i, idea) in TEST_SET.iter().enumerate() {
        let n = i + 1;
        let outline = generate_outline(client, base_url, api_key, model, style, idea).await?;
        let fulltext =
            render_fulltext(client, base_url, api_key, model, style, &outline, idea).await?;

        let quoted_idea: String = idea
            .split('\n')
            .map(|l| format!("> {}", l))
            .collect::<Vec<_>>()
            .join("\n");
        let content = format!(
            "# 主题 {}：{}\n\n{}\n\n## 大纲\n\n{}\n\n## 正文\n\n{}",
            n,
            idea.lines()
                .next()
                .unwrap_or("")
                .trim_start_matches("文章主题："),
            quoted_idea,
            outline,
            fulltext,
        );
        let path = format!("{}/topic_{:02}.md", target_dir, n);
        std::fs::write(&path, content)
            .map_err(|e| AppError::Intent(format!("写文件失败: {}", e)))?;
        println!(
            "[{}] 主题 {} 已保存 → {} (正文 {} 字符)",
            target_dir,
            n,
            path,
            fulltext.chars().count()
        );
    }

    Ok(())
}

/// 跑整批样文测试集，提取自由格式执行 Spec 并存到 target_dir。
pub async fn eval_learn_batch(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    target_dir: &str,
) -> Result<(), AppError> {
    let target = std::path::Path::new(target_dir);
    if target.exists() && std::fs::read_dir(target)?.next().is_some() {
        return Err(AppError::FileWrite(
            "目标目录非空；使用新目录或可续跑的 eval_automated_suite".into(),
        ));
    }
    std::fs::create_dir_all(target_dir)
        .map_err(|e| AppError::Intent(format!("创建目录失败: {}", e)))?;

    for n in 1..=5 {
        let sample_path = format!("eval/samples/sample_{:02}.md", n);
        let sample_text = std::fs::read_to_string(&sample_path)
            .map_err(|e| AppError::Intent(format!("读取样文失败: {}", e)))?;

        let spec = crate::learn::extract_style_text(client, base_url, api_key, model, &sample_text)
            .await?;
        let out_path = format!("{}/spec_{:02}.md", target_dir, n);
        std::fs::write(&out_path, &spec)
            .map_err(|e| AppError::Intent(format!("写 Spec 失败: {}", e)))?;

        println!(
            "[{}] 样文 {} 已提取 → {} ({} 字符)",
            target_dir,
            n,
            out_path,
            spec.chars().count()
        );
    }

    Ok(())
}
