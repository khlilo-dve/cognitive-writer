use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

// ── Clipboard injection ─────────────────────────────────────────────

/// 构建 Windows CF_HTML 格式：带字节偏移量的标准头部 + HTML 片段
/// 规范参考: https://learn.microsoft.com/en-us/windows/win32/dataxchg/html-clipboard-format
pub fn build_cf_html(fragment: &str) -> String {
    let pre = "<html>\r\n<head><meta charset=\"utf-8\"></head>\r\n<body>\r\n<!--StartFragment-->";
    let post = "<!--EndFragment-->\r\n</body>\r\n</html>";

    // 头部模板长度固定（全 ASCII，len() == 字节数）
    let header_len = "Version:0.9\r\nStartHTML:0000000000\r\nEndHTML:0000000000\r\nStartFragment:0000000000\r\nEndFragment:0000000000\r\n".len();

    let start_html = header_len;
    let start_frag = start_html + pre.len();
    let end_frag = start_frag + fragment.len(); // Rust .len() 返回字节数，CF_HTML 要求字节偏移
    let end_html = end_frag + post.len();

    format!(
        "Version:0.9\r\nStartHTML:{:010}\r\nEndHTML:{:010}\r\nStartFragment:{:010}\r\nEndFragment:{:010}\r\n{}{}{}",
        start_html, end_html, start_frag, end_frag, pre, fragment, post
    )
}

/// WSL2 → Windows: 通过 PowerShell + .NET System.Windows.Forms 写入 CF_HTML
pub fn inject_cf_html_powershell(html_fragment: &str) -> Result<&'static str, String> {
    let cf_html = build_cf_html(html_fragment);

    // 写入临时文件（UTF-8 无 BOM，Rust 默认行为）
    let tmp = "/tmp/cw_clipboard.html";
    fs::write(tmp, &cf_html).map_err(|e| format!("临时文件写入失败: {}", e))?;

    // 转换为 Windows 路径
    let wslpath_out = Command::new("wslpath")
        .args(["-w", tmp])
        .output()
        .map_err(|e| format!("wslpath 失败: {}", e))?;

    if !wslpath_out.status.success() {
        let _ = fs::remove_file(tmp);
        return Err("wslpath 路径转换失败".to_string());
    }

    let win_path = String::from_utf8_lossy(&wslpath_out.stdout).trim().to_string();

    // PowerShell 脚本：用 ReadAllBytes 读取原始 UTF-8 字节 → MemoryStream → CF_HTML
    // 关键：绕过 .NET String(UTF-16) 转换，避免系统默认编码(GBK)截断多字节中文
    let ps_script = format!(
        concat!(
            "Add-Type -AssemblyName System.Windows.Forms; ",
            "$bytes = [System.IO.File]::ReadAllBytes('{}'); ",
            "$ms = New-Object System.IO.MemoryStream(,$bytes); ",
            "$d = New-Object System.Windows.Forms.DataObject; ",
            "$d.SetData([System.Windows.Forms.DataFormats]::Html, $ms); ",
            "[System.Windows.Forms.Clipboard]::SetDataObject($d, $true)"
        ),
        win_path.replace('\'', "''")
    );

    let out = Command::new("powershell.exe")
        .args(["-sta", "-NoProfile", "-Command", &ps_script])
        .output()
        .map_err(|e| format!("powershell.exe 执行失败: {}", e))?;

    let _ = fs::remove_file(tmp);

    if out.status.success() {
        Ok("CF_HTML (PowerShell)")
    } else {
        Err(format!(
            "PowerShell 剪贴板设置失败: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// 通过 stdin pipe 向 CLI 工具写入数据
/// 修改：stderr 从 Stdio::null() 改为 Stdio::piped()，失败时将 stderr 内容包含在错误信息中
pub fn pipe_to_cmd(cmd: &str, args: &[&str], data: &[u8]) -> Result<(), String> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("无法启动 {}: {}", cmd, e))?;

    let stdin = child
        .stdin
        .as_mut()
        .ok_or_else(|| format!("无法打开 {} 的 stdin", cmd))?;
    stdin
        .write_all(data)
        .map_err(|e| format!("写入 {} 的 stdin 失败: {}", cmd, e))?;
    drop(child.stdin.take());

    let output = child
        .wait_with_output()
        .map_err(|e| format!("等待 {} 退出失败: {}", cmd, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let err_msg = if stderr.is_empty() {
            format!("{} 返回非零退出码", cmd)
        } else {
            format!("{} 失败: {}", cmd, stderr)
        };
        return Err(err_msg);
    }
    Ok(())
}

pub fn inject_clipboard(html_content: &str) -> Result<&'static str, String> {
    // 1. WSL2 / Windows: CF_HTML via PowerShell（真正的富文本，最优路径）
    match inject_cf_html_powershell(html_content) {
        Ok(tool) => return Ok(tool),
        Err(e) => eprintln!("[warn] PowerShell 剪贴板失败: {}", e),
    }

    // 2. Linux X11: xclip -selection clipboard -t text/html
    match pipe_to_cmd(
        "xclip",
        &["-selection", "clipboard", "-t", "text/html"],
        html_content.as_bytes(),
    ) {
        Ok(()) => return Ok("xclip (text/html)"),
        Err(e) => eprintln!("[warn] xclip 失败: {}", e),
    }

    // 3. Wayland: wl-copy --type text/html
    match pipe_to_cmd(
        "wl-copy",
        &["--type", "text/html"],
        html_content.as_bytes(),
    ) {
        Ok(()) => return Ok("wl-copy (text/html)"),
        Err(e) => eprintln!("[warn] wl-copy 失败: {}", e),
    }

    Err("未找到可用的剪贴板工具 (powershell.exe / xclip / wl-copy)".to_string())
}
