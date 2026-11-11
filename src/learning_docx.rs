//! Visible Word body text only; extraction never follows links or reads attached objects.
use crate::{error::AppError, learning_sources::MAX_BYTES};
use roxmltree::{Document, Node, ParsingOptions};
use std::{
    collections::HashMap,
    io::{Cursor, Read},
};
pub(crate) struct Text {
    pub body: String,
    pub incomplete: bool,
}
fn error(s: &str) -> AppError {
    AppError::FileRead(s.into())
}
fn w(n: Node<'_, '_>, name: &str) -> bool {
    n.is_element()
        && n.tag_name().name() == name
        && matches!(
            n.tag_name().namespace(),
            Some(
                "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
                    | "http://purl.oclc.org/ooxml/wordprocessingml/main"
            )
        )
}
fn val<'a>(n: Node<'a, '_>, key: &str) -> Option<&'a str> {
    n.attributes()
        .find(|a| {
            a.name() == key
                && matches!(
                    a.namespace(),
                    Some(
                        "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
                            | "http://purl.oclc.org/ooxml/wordprocessingml/main"
                    )
                )
        })
        .map(|a| a.value())
}
fn xml(s: &str) -> Result<Document<'_>, AppError> {
    Document::parse_with_options(
        s,
        ParsingOptions {
            nodes_limit: 200000,
            ..Default::default()
        },
    )
    .map_err(|_| error("Word XML无效或节点超限，未交付部分正文"))
}
fn part(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    name: &str,
    required: bool,
) -> Result<String, AppError> {
    let file = match archive.by_name(name) {
        Ok(f) => f,
        Err(zip::result::ZipError::FileNotFound) if !required => return Ok(String::new()),
        Err(_) => return Err(error("DOCX缺少正文或文档包损坏/加密")),
    };
    if file.size() > MAX_BYTES as u64 {
        return Err(error("DOCX解压正文/XML超过8MiB，未截断"));
    }
    let mut bytes = vec![];
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| error("DOCX解压失败"))?;
    if bytes.len() > MAX_BYTES {
        return Err(error("DOCX解压XML超过8MiB"));
    }
    String::from_utf8(bytes).map_err(|_| error("DOCX XML不是UTF-8，未猜测编码"))
}
fn visible(n: Node<'_, '_>) -> bool {
    !n.ancestors().any(|a| {
        w(a, "del")
            || w(a, "moveFrom")
            || ((w(a, "r") || w(a, "p"))
                && a.children()
                    .filter(|c| w(*c, "rPr") || w(*c, "pPr"))
                    .flat_map(|p| p.descendants())
                    .any(|p| {
                        (w(p, "vanish") || w(p, "webHidden"))
                            && !matches!(val(p, "val"), Some("0" | "false" | "off"))
                    }))
    })
}
pub(crate) fn extract(bytes: &[u8]) -> Result<Text, AppError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| error("不是有效DOCX文档包；旧.doc请另存为DOCX或文本"))?;
    if archive.len() > 2048 {
        return Err(error("DOCX包条目超过2048，未处理"));
    }
    let raw = part(&mut archive, "word/document.xml", true)?;
    let styles = part(&mut archive, "word/styles.xml", false)?;
    let doc = xml(&raw)?;
    let body = doc
        .descendants()
        .find(|n| w(*n, "body"))
        .ok_or_else(|| error("DOCX没有Word正文节点"))?;
    let mut levels = HashMap::new();
    if !styles.is_empty() {
        let styles = xml(&styles)?;
        for style in styles.descendants().filter(|n| w(*n, "style")) {
            if let Some(id) = val(style, "styleId") {
                if let Some(level) = style
                    .descendants()
                    .find(|n| w(*n, "outlineLvl"))
                    .and_then(|n| val(n, "val"))
                    .and_then(|s| s.parse::<usize>().ok())
                    .filter(|n| *n < 6)
                {
                    levels.insert(id.to_string(), level + 1);
                }
            }
        }
    }
    let incomplete = body.descendants().any(|n| {
        visible(n)
            && (w(n, "drawing")
                || w(n, "pict")
                || w(n, "object")
                || w(n, "altChunk")
                || w(n, "txbxContent")
                || w(n, "sym")
                || n.tag_name().namespace()
                    == Some("http://schemas.openxmlformats.org/officeDocument/2006/math"))
    });
    let mut out = String::new();
    for p in body
        .descendants()
        .filter(|n| w(*n, "p") && visible(*n) && !n.ancestors().any(|a| w(a, "txbxContent")))
    {
        let mut paragraph = String::new();
        for n in p.descendants().filter(|n| visible(*n)) {
            if w(n, "t") {
                paragraph.push_str(n.text().unwrap_or(""));
            } else if w(n, "tab") {
                paragraph.push('\t');
            } else if w(n, "br") || w(n, "cr") {
                paragraph.push('\n');
            }
        }
        if paragraph.trim().is_empty() {
            continue;
        }
        let properties = p.children().find(|n| w(*n, "pPr"));
        let explicit = properties
            .and_then(|p| p.children().find(|n| w(*n, "outlineLvl")))
            .and_then(|n| val(n, "val"))
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|n| *n < 6)
            .map(|n| n + 1);
        let style = properties
            .and_then(|p| p.children().find(|n| w(*n, "pStyle")))
            .and_then(|n| val(n, "val"));
        let level = explicit
            .or_else(|| style.and_then(|s| levels.get(s).copied()))
            .or_else(|| {
                style
                    .and_then(|s| s.strip_prefix("Heading").or_else(|| s.strip_prefix("标题")))
                    .and_then(|s| s.parse::<usize>().ok())
                    .filter(|n| (1..=6).contains(n))
            });
        if let Some(n) = level {
            out.push_str(&"#".repeat(n));
            out.push(' ');
        }
        out.push_str(paragraph.trim());
        out.push_str("\n\n");
        if out.len() > MAX_BYTES {
            return Err(error("DOCX提取正文超过8MiB，未截断"));
        }
    }
    Ok(Text {
        body: out.trim().into(),
        incomplete,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn file(body: &str) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(vec![]));
        zip.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        write!(zip,"<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}</w:body></w:document>").unwrap();
        zip.finish().unwrap().into_inner()
    }
    #[test]
    fn visible_runs_headings_tables_entities_and_revisions_preserve_order() {
        let v = file(
            "<w:p><w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr><w:r><w:t>标题</w:t></w:r></w:p><w:p><w:r><w:t xml:space=\"preserve\">甲 &amp; </w:t></w:r><w:hyperlink><w:r><w:t>乙</w:t></w:r></w:hyperlink><w:del><w:r><w:t>删</w:t></w:r></w:del><w:r><w:rPr><w:vanish/></w:rPr><w:t>隐藏</w:t></w:r><w:r><w:rPr><w:vanish w:val=\"0\"/></w:rPr><w:t>可见</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>表格</w:t></w:r></w:p></w:tc></w:tr></w:tbl>",
        );
        let got = extract(&v).unwrap();
        assert_eq!(got.body, "# 标题\n\n甲 & 乙可见\n\n表格");
        assert!(!got.incomplete);
    }
    #[test]
    fn embedded_objects_and_invalid_xml_are_not_silently_complete() {
        assert!(
            extract(&file("<w:p><w:r><w:t>说明</w:t><w:drawing/></w:r></w:p>"))
                .unwrap()
                .incomplete
        );
        assert!(extract(&file("<w:p>")).is_err());
        assert!(extract(b"not zip").is_err());
    }
}
