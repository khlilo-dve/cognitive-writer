//! Structural article candidates; scope/voice decisions belong to the learning agent.
use crate::error::AppError;
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};
use scraper::{ElementRef, Html, Node, Selector};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageLink {
    pub title: String,
    pub url: String,
}
#[derive(Debug)]
pub(crate) struct ArticleText {
    pub title: String,
    pub text: String,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PageKind {
    Articles,
    NeedsInspection,
    NonArticle,
}
pub(crate) struct Extraction {
    pub kind: PageKind,
    pub articles: Vec<ArticleText>,
    pub links: Vec<PageLink>,
    pub method: String,
    pub reason: String,
}
fn selector(css: &str) -> Result<Selector, AppError> {
    Selector::parse(css).map_err(|_| AppError::Parse("内部正文选择器无效".into()))
}
fn plain(kind: PageKind, reason: &str) -> Extraction {
    Extraction {
        kind,
        articles: vec![],
        links: vec![],
        method: "unresolved".into(),
        reason: reason.into(),
    }
}

pub(crate) fn text(raw: &str, label: &str) -> Extraction {
    let raw = raw.trim_start_matches('\u{feff}').trim();
    if raw.is_empty() {
        return plain(PageKind::NonArticle, "正文为空");
    }
    let mut headings = Vec::new();
    let mut heading = None;
    for (event, range) in Parser::new(raw).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading {
                level: HeadingLevel::H1,
                ..
            }) => heading = Some((range.start, String::new())),
            Event::Text(s) | Event::Code(s) => {
                if let Some((_, title)) = &mut heading {
                    title.push_str(&s);
                }
            }
            Event::End(TagEnd::Heading(HeadingLevel::H1)) => {
                if let Some(h) = heading.take() {
                    headings.push(h);
                }
            }
            _ => {}
        }
    }
    let articles = if headings.len() > 1 {
        let mut result = Vec::new();
        if !raw[..headings[0].0].trim().is_empty() {
            result.push(ArticleText {
                title: "未归属前文".into(),
                text: raw[..headings[0].0].trim().into(),
            });
        }
        for (i, (start, title)) in headings.iter().enumerate() {
            let end = headings.get(i + 1).map(|h| h.0).unwrap_or(raw.len());
            result.push(ArticleText {
                title: title.clone(),
                text: raw[*start..end].trim().into(),
            });
        }
        result
    } else {
        vec![ArticleText {
            title: headings
                .first()
                .map(|h| h.1.as_str())
                .unwrap_or(label)
                .into(),
            text: raw.into(),
        }]
    };
    Extraction {
        kind: PageKind::Articles,
        reason: if articles.len() > 1 {
            "多个一级标题，文章边界需选择确认".into()
        } else {
            String::new()
        },
        articles,
        links: vec![],
        method: "provided_text_or_markdown".into(),
    }
}

fn excluded(el: ElementRef<'_>) -> bool {
    matches!(
        el.value().name(),
        "script"
            | "style"
            | "nav"
            | "footer"
            | "aside"
            | "form"
            | "button"
            | "noscript"
            | "head"
            | "select"
            | "template"
    ) || (el.value().name() == "header"
        && !el
            .ancestors()
            .filter_map(ElementRef::wrap)
            .any(|a| a.value().name() == "article"))
        || el.attr("hidden").is_some()
        || el.attr("aria-hidden") == Some("true")
}
fn walk(el: ElementRef<'_>, out: &mut String, depth: usize) -> Result<(), AppError> {
    if depth > 128 {
        return Err(AppError::Parse(
            "正文DOM嵌套超过128层，未交付截断文本".into(),
        ));
    }
    if excluded(el) {
        return Ok(());
    }
    let block = matches!(
        el.value().name(),
        "p" | "div"
            | "section"
            | "article"
            | "main"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "li"
            | "blockquote"
            | "pre"
            | "br"
            | "hr"
    );
    if block {
        out.push('\n');
    }
    for child in el.children() {
        match child.value() {
            Node::Text(s) => out.push_str(s),
            Node::Element(_) => {
                if let Some(child) = ElementRef::wrap(child) {
                    walk(child, out, depth + 1)?;
                }
            }
            _ => {}
        }
    }
    if block {
        out.push('\n');
    }
    Ok(())
}
fn visible(el: ElementRef<'_>) -> Result<String, AppError> {
    let mut out = String::new();
    walk(el, &mut out, 0)?;
    Ok(out
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n"))
}
fn title(el: ElementRef<'_>, fallback: &str) -> Result<String, AppError> {
    match el.select(&selector("h1, h2")?).next() {
        Some(h) => visible(h),
        None => Ok(fallback.into()),
    }
}
fn has_prose(el: ElementRef<'_>) -> Result<bool, AppError> {
    for paragraph in el.select(&selector("p")?) {
        let all = visible(paragraph)?;
        let links = paragraph
            .select(&selector("a")?)
            .map(|a| visible(a))
            .collect::<Result<Vec<_>, _>>()?
            .join("");
        let count = |s: &str| s.chars().filter(|c| !c.is_whitespace()).count();
        if count(&all) > count(&links) {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn html(raw: &str, url: &str) -> Result<Extraction, AppError> {
    let doc = Html::parse_document(raw);
    let root = doc.root_element();
    let fallback = match doc.select(&selector("title")?).next() {
        Some(t) => visible(t)?,
        None => "未命名正文".into(),
    };
    let base = reqwest::Url::parse(url).ok();
    let mut links = Vec::new();
    for a in doc.select(&selector("a[href]")?) {
        let Some(target) = base
            .as_ref()
            .and_then(|b| b.join(a.attr("href").unwrap_or("")).ok())
        else {
            continue;
        };
        let label = visible(a)?;
        if matches!(target.scheme(), "http" | "https")
            && !label.is_empty()
            && target.username().is_empty()
            && target.password().is_none()
        {
            links.push(PageLink {
                title: label,
                url: target.into(),
            });
        }
    }
    let article_selector = selector("article, [role=article]")?;
    let articles: Vec<_> = doc
        .select(&article_selector)
        .filter(|el| {
            !el.ancestors()
                .filter_map(ElementRef::wrap)
                .any(|a| article_selector.matches(&a))
        })
        .collect();
    let (containers, method) = if !articles.is_empty() {
        (articles, "html_article")
    } else {
        let strong = selector("[itemprop=articleBody], #js_content, .rich_media_content")?;
        let nodes: Vec<_> = doc
            .select(&strong)
            .filter(|el| {
                !el.ancestors()
                    .filter_map(ElementRef::wrap)
                    .any(|a| strong.matches(&a))
            })
            .collect();
        if !nodes.is_empty() {
            (nodes, "html_article_body")
        } else {
            (
                doc.select(&selector("main, [role=main]")?)
                    .filter(|el| {
                        !el.ancestors().filter_map(ElementRef::wrap).any(|a| {
                            matches!(a.value().name(), "main") || a.attr("role") == Some("main")
                        })
                    })
                    .collect(),
                "html_main",
            )
        }
    };
    let mut candidates = Vec::new();
    let mut incomplete = false;
    for container in containers {
        if excluded(container) {
            continue;
        }
        let body = visible(container)?;
        if body.is_empty() {
            continue;
        }
        // A semantic container is stronger evidence than a generic body; main still needs prose.
        if method == "html_main" && !has_prose(container)? {
            continue;
        }
        let heading = title(container, &fallback)?;
        incomplete |= body == heading;
        for link in container.select(&selector("a[href]")?) {
            let label = visible(link)?.to_ascii_lowercase();
            incomplete |= matches!(label.trim(), "阅读全文" | "继续阅读" | "read more");
        }
        candidates.push(ArticleText {
            title: heading,
            text: body,
        });
    }
    let mut result = if !candidates.is_empty() {
        Extraction {
            kind: if incomplete {
                PageKind::NeedsInspection
            } else {
                PageKind::Articles
            },
            articles: candidates,
            links: vec![],
            method: method.into(),
            reason: if incomplete {
                "容器可能只有标题或摘要入口，尚未确认全文".into()
            } else {
                String::new()
            },
        }
    } else if doc
        .select(&selector("input[type=password]")?)
        .next()
        .is_some()
    {
        plain(PageKind::NonArticle, "登录表单，未取得文章正文")
    } else if root.select(&selector("h1")?).count() == 1 && has_prose(root)? {
        // Generic pages need domain review; paragraph presence alone cannot prove article boundaries.
        Extraction {
            kind: PageKind::NeedsInspection,
            articles: vec![ArticleText {
                title: fallback,
                text: visible(root)?,
            }],
            links: vec![],
            method: "html_unconfirmed_body".into(),
            reason: "存在正文段落但缺少明确文章容器，待领域识别".into(),
        }
    } else if !links.is_empty() || doc.select(&selector("form, nav")?).next().is_some() {
        plain(
            PageKind::NonArticle,
            "导航、目录或表单页面，未取得明确文章正文",
        )
    } else {
        plain(PageKind::NeedsInspection, "未能确定文章正文边界")
    };
    result.links = links;
    Ok(result)
}

pub(crate) fn reader(raw: &str) -> Extraction {
    if let Some((head, content)) = raw.split_once("Markdown Content:\n") {
        if head.contains("URL Source:") {
            let label = head
                .lines()
                .find_map(|l| l.strip_prefix("Title:"))
                .unwrap_or("Reader正文")
                .trim();
            let mut parsed = text(content, label);
            parsed.method = "reader_markdown".into();
            // Markdown without an HTML boundary is a candidate requiring semantic inspection.
            parsed.kind = PageKind::NeedsInspection;
            parsed.reason = "Reader正文候选需领域识别，未据HTTP成功认定文章".into();
            return parsed;
        }
    }
    plain(
        PageKind::NeedsInspection,
        "备用读取缺少可核对的Reader正文边界",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn html_preserves_inline_and_paragraphs_but_not_scripts_or_navigation() {
        let p=html("<title>样文</title><nav>菜单</nav><article><h1>标题</h1><p>汉<em>字</em>&amp;句。</p><script>盗用指令</script><p>第二段。</p><aside>推荐</aside><p hidden>隐藏</p></article>","https://example.org/a").unwrap();
        assert_eq!(p.kind, PageKind::Articles);
        assert_eq!(p.articles[0].text, "标题\n汉字&句。\n第二段。");
    }
    #[test]
    fn multiple_articles_and_heading_sections_remain_separate_candidates() {
        let p=html("<article><h1>甲</h1><p>甲文。</p><article>嵌套补充。</article></article><article><h1>乙</h1><p>乙文。</p></article>","https://example.org").unwrap();
        assert_eq!(p.articles.len(), 2);
        assert!(p.articles[0].text.contains("嵌套补充"));
        let p = text("前言。\n\n# 第一篇\n\n甲文。\n\n# 第二篇\n\n乙文。", "文件");
        assert_eq!(p.articles.len(), 3);
        assert_eq!(p.articles[0].text, "前言。");
        assert!(p.articles[2].text.contains("乙文。"));
    }
    #[test]
    fn login_directory_generic_page_and_short_valid_article_are_distinct() {
        assert_eq!(
            html("<form><input type=password></form>", "https://example.org")
                .unwrap()
                .kind,
            PageKind::NonArticle
        );
        let p = html(
            "<nav><a href='/one'>一篇</a><a href='/two'>二篇</a></nav>",
            "https://example.org/list",
        )
        .unwrap();
        assert_eq!(p.kind, PageKind::NonArticle);
        assert_eq!(p.links[1].url, "https://example.org/two");
        assert_eq!(
            html(
                "<h1>标题</h1><p>正文没有明确容器。</p>",
                "https://example.org"
            )
            .unwrap()
            .kind,
            PageKind::NeedsInspection
        );
        assert_eq!(
            html("<article><p>短。</p></article>", "https://example.org")
                .unwrap()
                .kind,
            PageKind::Articles
        );
        assert_eq!(text("短。", "短文").articles[0].text, "短。");
    }
    #[test]
    fn title_only_and_read_more_cards_require_internal_inspection() {
        for markup in [
            "<article><h1>只有标题</h1></article>",
            "<article><h1>文章卡片</h1><p>这只是摘要。</p><a href='/full'>阅读全文</a></article>",
        ] {
            let parsed = html(markup, "https://example.org").unwrap();
            assert_eq!(parsed.kind, PageKind::NeedsInspection);
            assert_eq!(parsed.articles.len(), 1);
        }
    }
    #[test]
    fn reader_metadata_is_not_an_automatic_article_certificate() {
        let p = reader(
            "Title: 页面\nURL Source: https://example.org\nMarkdown Content:\n# 登录\n请输入密码",
        );
        assert_eq!(p.kind, PageKind::NeedsInspection);
        assert!(!p.articles[0].text.contains("URL Source"));
    }
}
