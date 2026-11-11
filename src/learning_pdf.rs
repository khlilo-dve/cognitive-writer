//! PDF text layer, page by page. Errors on later pages never become a successful prefix.
use crate::{error::AppError, learning_sources::MAX_BYTES};
use pdf_extract::{Document, PlainTextOutput};
use std::io::{self, Write};
struct Bounded {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for Bounded {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.bytes.len().saturating_add(buf.len()) > self.limit {
            return Err(io::Error::other("PDF提取文字超限"));
        }
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[derive(Debug)]
pub(crate) struct Text {
    pub body: String,
    pub pages: usize,
    pub blank_pages: Vec<u32>,
}
fn error(s: &str) -> AppError {
    AppError::FileRead(s.into())
}
pub(crate) fn extract(bytes: &[u8]) -> Result<Text, AppError> {
    std::panic::catch_unwind(|| extract_inner(bytes))
        .map_err(|_| error("PDF结构或字体不受支持，未交付部分正文"))?
}
fn extract_inner(bytes: &[u8]) -> Result<Text, AppError> {
    let mut doc = Document::load_mem(bytes).map_err(|_| error("PDF损坏或结构不受支持"))?;
    if doc.is_encrypted() {
        return Err(error("PDF已加密，请提供未加密文字版或TXT/DOCX"));
    }
    unicode_cmaps(&mut doc)?;
    let pages = doc.get_pages();
    if pages.is_empty() || pages.len() > 200 {
        return Err(error("PDF需要1–200页，超限请提供目标文章文件"));
    }
    let mut out = String::new();
    let mut blank = vec![];
    for (&number, &id) in &pages {
        let content = doc
            .get_page_content(id)
            .map_err(|_| error("PDF页面内容读取失败，未交付前面部分"))?;
        if content.len() > MAX_BYTES {
            return Err(error("PDF单页解压内容超过8MiB，未处理"));
        }
        let mut writer = Bounded {
            bytes: vec![],
            limit: MAX_BYTES.saturating_sub(out.len()),
        };
        {
            let mut output = PlainTextOutput::new(&mut writer as &mut dyn Write);
            pdf_extract::output_doc_page(&doc, &mut output, number)
                .map_err(|_| error("PDF页面文字提取失败或超8MiB，未交付部分正文"))?;
        }
        let text = String::from_utf8(writer.bytes).map_err(|_| error("PDF文字编码无效"))?;
        if text.contains('�') || text.contains('\0') {
            return Err(error("PDF有无法映射的文字，请提供可读文字版"));
        }
        if text.trim().is_empty() {
            let image = doc
                .get_page_images(id)
                .map(|a| !a.is_empty())
                .unwrap_or(false);
            let ops = pdf_extract::content::Content::decode(&content)
                .map_err(|_| error("PDF空文字页面指令无效，未忽略该页"))?;
            let painted = ops.operations.iter().any(|o| {
                matches!(
                    o.operator.as_str(),
                    "Do" | "BI"
                        | "ID"
                        | "Tj"
                        | "TJ"
                        | "'"
                        | "\""
                        | "S"
                        | "s"
                        | "f"
                        | "F"
                        | "f*"
                        | "B"
                        | "B*"
                        | "b"
                        | "b*"
                        | "sh"
                )
            });
            if image || painted {
                return Err(error(&format!(
                    "PDF第{number}页含绘制内容但没有可提取文字，可能为扫描页；不支持OCR，未遗漏该页继续学习"
                )));
            }
            blank.push(number);
            continue;
        }
        out.push_str(text.trim());
        out.push_str("\n\n");
        if out.len() > MAX_BYTES {
            return Err(error("PDF提取正文超过8MiB，未截断"));
        }
    }
    if out.trim().is_empty() {
        return Err(error("PDF没有可提取文字，扫描件需提供文字版；未调用OCR"));
    }
    Ok(Text {
        body: out.trim().into(),
        pages: pages.len(),
        blank_pages: blank,
    })
}
// These predefined CMaps use UCS-2 character codes. Preserve original bytes and
// normalise only this in-memory extraction view; do not guess arbitrary CID mappings.
fn unicode_cmaps(doc: &mut Document) -> Result<(), AppError> {
    use pdf_extract::{Object, Stream, dictionary};
    let map=b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CMapType 2 def 1 begincodespacerange <0000> <FFFF> endcodespacerange 1 beginbfrange <0000> <FFFF> <0000> endbfrange endcmap end end";
    let mut unicode_id = None;
    let ids: Vec<_> = doc
        .objects
        .iter()
        .filter_map(|(id, o)| {
            let d = o.as_dict().ok()?;
            if d.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Type0") {
                return None;
            }
            let encoding = d.get(b"Encoding").and_then(Object::as_name).ok()?;
            let known = [
                b"UniGB-UCS2-H".as_slice(),
                b"UniGB-UCS2-V",
                b"UniCNS-UCS2-H",
                b"UniCNS-UCS2-V",
                b"UniJIS-UCS2-H",
                b"UniJIS-UCS2-V",
                b"UniKS-UCS2-H",
                b"UniKS-UCS2-V",
            ]
            .contains(&encoding);
            known.then_some((*id, encoding.ends_with(b"V"), d.has(b"ToUnicode")))
        })
        .collect();
    for (id, vertical, has_unicode) in ids {
        let new_map =
            if has_unicode {
                None
            } else {
                Some(*unicode_id.get_or_insert_with(|| {
                    doc.add_object(Stream::new(dictionary! {}, map.to_vec()))
                }))
            };
        if let Ok(d) = doc.get_object_mut(id).and_then(Object::as_dict_mut) {
            d.set(
                "Encoding",
                if vertical { "Identity-V" } else { "Identity-H" },
            );
            if let Some(id) = new_map {
                d.set("ToUnicode", id);
            }
        }
    }
    for object in doc.objects.values() {
        if let Ok(d) = object.as_dict() {
            if d.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Type0") {
                if !d.has(b"ToUnicode") {
                    return Err(error(
                        "PDF复合字体缺少Unicode映射，不能把字形编号猜成文字，请提供正文版",
                    ));
                }
                if let Ok(name) = d.get(b"Encoding").and_then(Object::as_name) {
                    if name != b"Identity-H" && name != b"Identity-V" {
                        return Err(error("PDF字体编码不受支持，请提供可读文字版"));
                    }
                }
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn pdf(contents: &[&str]) -> Vec<u8> {
        use pdf_extract::{Object, Stream, dictionary};
        let mut doc = Document::with_version("1.5");
        let pages = doc.new_object_id();
        let font =
            doc.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica"});
        let mut kids = vec![];
        for text in contents {
            let stream = doc.add_object(Stream::new(dictionary! {}, text.as_bytes().to_vec()));
            let page=doc.add_object(dictionary!{"Type"=>"Page","Parent"=>pages,"MediaBox"=>vec![0.into(),0.into(),600.into(),800.into()],"Resources"=>dictionary!{"Font"=>dictionary!{"F1"=>font}},"Contents"=>stream});
            kids.push(Object::Reference(page));
        }
        doc.objects.insert(
            pages,
            Object::Dictionary(
                dictionary! {"Type"=>"Pages","Count"=>kids.len() as i64,"Kids"=>kids},
            ),
        );
        let catalog = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
        doc.trailer.set("Root", catalog);
        let mut bytes = vec![];
        doc.save_to(&mut bytes).unwrap();
        bytes
    }
    #[test]
    fn pages_are_one_text_and_blank_pages_not_missing_articles() {
        let p = extract(&pdf(&[
            "BT /F1 12 Tf 50 700 Td (First page.) Tj ET",
            "",
            "BT /F1 12 Tf 50 700 Td (Last page.) Tj ET",
        ]))
        .unwrap();
        assert!(p.body.find("First page.").unwrap() < p.body.find("Last page.").unwrap());
        assert_eq!(p.pages, 3);
        assert_eq!(p.blank_pages, vec![2]);
    }
    #[test]
    fn non_text_page_does_not_return_first_page_as_complete() {
        assert!(
            extract(&pdf(&[
                "BT /F1 12 Tf 50 700 Td (Text.) Tj ET",
                "0 0 100 100 re f"
            ]))
            .unwrap_err()
            .to_string()
            .contains("第2页")
        );
        assert!(extract(b"broken").is_err());
    }
}
