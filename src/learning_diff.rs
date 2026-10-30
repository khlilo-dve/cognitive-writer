//! Bounded line diff. Large inputs fall back to an exact changed span, without truncating text.
pub(crate) fn diff(before: &str, after: &str) -> String {
    if before == after {
        return "说明文字未变化。\n".into();
    }
    let a = before.lines().collect::<Vec<_>>();
    let b = after.lines().collect::<Vec<_>>();
    let mut prefix = 0;
    while prefix < a.len().min(b.len()) && a[prefix] == b[prefix] {
        prefix += 1;
    }
    if prefix == a.len() && prefix == b.len() {
        return "说明只有换行符或末尾换行变化（正文行内容相同）。\n".into();
    }
    let mut suffix = 0;
    while suffix < (a.len() - prefix).min(b.len() - prefix)
        && a[a.len() - suffix - 1] == b[b.len() - suffix - 1]
    {
        suffix += 1;
    }
    let left = &a[prefix..a.len() - suffix];
    let right = &b[prefix..b.len() - suffix];
    let mut output = format!("--- 上一版本\n+++ 当前候选\n@@ 从第{}行起 @@\n", prefix + 1);
    if left.len().saturating_mul(right.len()) > 250_000 {
        for line in left {
            output.push_str(&format!("-{line}\n"));
        }
        for line in right {
            output.push_str(&format!("+{line}\n"));
        }
    } else {
        let mut lengths = vec![vec![0; right.len() + 1]; left.len() + 1];
        for i in (0..left.len()).rev() {
            for j in (0..right.len()).rev() {
                lengths[i][j] = if left[i] == right[j] {
                    1 + lengths[i + 1][j + 1]
                } else {
                    lengths[i + 1][j].max(lengths[i][j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < left.len() || j < right.len() {
            if i < left.len() && j < right.len() && left[i] == right[j] {
                output.push_str(&format!(" {}\n", left[i]));
                i += 1;
                j += 1;
            } else if j < right.len() && (i == left.len() || lengths[i][j + 1] >= lengths[i + 1][j])
            {
                output.push_str(&format!("+{}\n", right[j]));
                j += 1;
            } else {
                output.push_str(&format!("-{}\n", left[i]));
                i += 1;
            }
        }
    }
    if before.ends_with('\n') != after.ends_with('\n') {
        output.push_str("末尾换行状态不同。\n");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interleaved_unicode_changes_and_large_fallback_preserve_all_modified_lines() {
        let result = diff(
            "开头\n旧甲\n相同\n旧乙\n结尾",
            "开头\n新甲\n相同\n新乙\n结尾",
        );
        for line in ["-旧甲", "+新甲", " 相同", "-旧乙", "+新乙"] {
            assert!(result.contains(line));
        }
        let result = diff(&"旧行\n".repeat(600), &"新行\n".repeat(600));
        assert_eq!(result.matches("-旧行").count(), 600);
        assert_eq!(result.matches("+新行").count(), 600);
        assert!(diff("原文", "原文\n").contains("末尾换行"));
        assert!(diff("原文\r\n", "原文\n").contains("换行符"));
    }
}
