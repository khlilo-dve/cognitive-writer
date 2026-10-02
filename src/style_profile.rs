//! Research-only source-grounded profiles; counts are computed, never accepted from the model.
use crate::{
    error::AppError,
    research_dataset::valid_id,
    style_compare::{Feature, View, DIMENSIONS},
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Source {
    pub id: String,
    pub article_id: String,
    pub topic: String,
    pub text: String,
    pub view: View,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Evidence {
    pub article: String,
    pub status: String,
    pub feature_ids: Vec<String>,
    pub quote: String,
    pub reason: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Rule {
    pub id: String,
    pub dimension: String,
    pub rule: String,
    pub condition: String,
    pub evidence: Vec<Evidence>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelProfile {
    rules: Vec<Rule>,
    omissions: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct ProfileRule {
    pub candidate: Rule,
    pub classification: String,
    pub support_articles: Vec<String>,
    pub counter_articles: Vec<String>,
    pub unknown_articles: Vec<String>,
    pub applicable_n: usize,
    pub support_ratio: Option<f64>,
    pub support_topics: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Profile {
    pub rules: Vec<ProfileRule>,
    pub omissions: Vec<String>,
}

pub(crate) fn parse_indexed(raw: &str, sources: &[Source]) -> Result<Profile, AppError> {
    let mut value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| AppError::Parse(format!("行引用Profile JSON: {e}")))?;
    let rules = value["rules"]
        .as_array_mut()
        .ok_or_else(|| AppError::Parse("缺少Profile rules".into()))?;
    for rule in rules {
        for e in rule["evidence"]
            .as_array_mut()
            .ok_or_else(|| AppError::Parse("缺少逐篇证据".into()))?
        {
            let source = sources
                .iter()
                .find(|s| e["article"] == s.id)
                .ok_or_else(|| AppError::Parse("引用非当前子集".into()))?;
            let start = e["line_start"]
                .as_u64()
                .ok_or_else(|| AppError::Parse("缺少line_start".into()))?
                as usize;
            let end = e["line_end"]
                .as_u64()
                .ok_or_else(|| AppError::Parse("缺少line_end".into()))?
                as usize;
            let quote = if e["status"] == "unknown" {
                if start != 0 || end != 0 {
                    return Err(AppError::Parse("unknown行号须为0".into()));
                }
                String::new()
            } else {
                let lines: Vec<_> = source.text.split('\n').collect();
                if start == 0 || end < start || end > lines.len() || end - start > 2 {
                    return Err(AppError::Parse("原文引用行号越界/倒序/超过3行".into()));
                }
                lines[start - 1..end].join("\n").trim().to_owned()
            };
            let object = e
                .as_object_mut()
                .ok_or_else(|| AppError::Parse("证据须为对象".into()))?;
            if object.contains_key("quote") {
                return Err(AppError::Parse("请仅引用行号，不重抄quote".into()));
            }
            object.remove("line_start");
            object.remove("line_end");
            object.insert("quote".into(), serde_json::json!(quote));
        }
    }
    parse(&value.to_string(), sources)
}

pub(crate) fn parse(raw: &str, sources: &[Source]) -> Result<Profile, AppError> {
    let input: ModelProfile =
        serde_json::from_str(raw).map_err(|e| AppError::Parse(format!("Profile JSON: {e}")))?;
    let articles: HashMap<_, _> = sources.iter().map(|s| (s.id.as_str(), s)).collect();
    let unique_articles: HashSet<_> = sources.iter().map(|s| &s.article_id).collect();
    if articles.len() != sources.len()
        || unique_articles.len() != sources.len()
        || sources.is_empty()
        || input.rules.len() > 12
    {
        return Err(AppError::Parse("Profile来源重复/为空或超过12条规则".into()));
    }
    let mut ids = HashSet::new();
    let mut rules = Vec::new();
    for rule in input.rules {
        if !valid_id(&rule.id)
            || !ids.insert(rule.id.clone())
            || !DIMENSIONS.contains(&rule.dimension.as_str())
            || rule.rule.trim().is_empty()
            || rule.condition.trim().is_empty()
            || rule.evidence.len() != sources.len()
        {
            return Err(AppError::Parse(
                "Profile规则ID/维度/条件/逐篇覆盖无效".into(),
            ));
        }
        let mut seen = HashSet::new();
        let (mut support, mut counter, mut unknown, mut topics) = (vec![], vec![], vec![], vec![]);
        for e in &rule.evidence {
            let s = articles
                .get(e.article.as_str())
                .ok_or_else(|| AppError::Parse("引用非当前子集文章".into()))?;
            if !seen.insert(&e.article) || e.reason.trim().is_empty() {
                return Err(AppError::Parse("逐篇证据重复或缺少理由".into()));
            }
            let mut features = HashSet::new();
            for id in &e.feature_ids {
                if !features.insert(id)
                    || !s
                        .view
                        .features
                        .iter()
                        .any(|f| f.id == *id && f.dimension == rule.dimension)
                {
                    return Err(AppError::Parse("证据feature_id不存在/重复/跨维度".into()));
                }
            }
            if e.status != "unknown" && (e.quote.chars().count() < 6 || !s.text.contains(&e.quote))
            {
                return Err(AppError::Parse("原文证据无法逐字定位".into()));
            }
            match e.status.as_str() {
                "support" if !e.feature_ids.is_empty() => {
                    support.push(s.article_id.clone());
                    topics.push(s.topic.clone());
                }
                "counter" => counter.push(s.article_id.clone()),
                "unknown" if e.quote.is_empty() => unknown.push(s.article_id.clone()),
                _ => {
                    return Err(AppError::Parse(
                        "证据状态无效，support须有Style依据，unknown不伪造引用".into(),
                    ))
                }
            }
        }
        support.sort();
        counter.sort();
        unknown.sort();
        topics.sort();
        topics.dedup();
        let n = support.len() + counter.len();
        let ratio = (n > 0).then(|| support.len() as f64 / n as f64);
        let class = if !counter.is_empty() {
            "conflict"
        } else if n >= 5 && ratio.unwrap_or(0.0) >= 0.8 && topics.len() >= 2 {
            "stable"
        } else if n >= 3 && ratio.unwrap_or(0.0) >= 0.5 {
            "weak"
        } else if support.len() == 1 && sources.len() >= 5 {
            "occasional"
        } else {
            "unknown"
        };
        rules.push(ProfileRule {
            candidate: rule,
            classification: class.into(),
            support_articles: support,
            counter_articles: counter,
            unknown_articles: unknown,
            applicable_n: n,
            support_ratio: ratio,
            support_topics: topics,
        });
    }
    Ok(Profile {
        rules,
        omissions: input.omissions,
    })
}

impl Profile {
    pub fn view(&self) -> View {
        View {
            features: self
                .rules
                .iter()
                .map(|r| Feature {
                    id: r.candidate.id.clone(),
                    dimension: r.candidate.dimension.clone(),
                    rule: r.candidate.rule.clone(),
                    condition: r.candidate.condition.clone(),
                    quote: r.candidate.rule.clone(),
                })
                .collect(),
            uncertainties: self.omissions.clone(),
        }
    }
    pub fn executable(&self) -> Vec<&ProfileRule> {
        self.rules
            .iter()
            .filter(|r| ["stable", "weak"].contains(&r.classification.as_str()))
            .collect()
    }
}

pub(crate) fn compile(raw: &str, profile: &Profile) -> Result<String, AppError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Section {
        feature_id: String,
        explanation: String,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Compiled {
        sections: Vec<Section>,
        evidence_insufficient: bool,
    }
    let doc: Compiled =
        serde_json::from_str(raw).map_err(|e| AppError::Parse(format!("执行说明JSON: {e}")))?;
    let allowed: HashMap<_, _> = profile
        .executable()
        .into_iter()
        .map(|r| (r.candidate.id.as_str(), r))
        .collect();
    let mut seen = HashSet::new();
    let mut md = String::from("# 写作声音执行说明\n\n");
    for section in doc.sections {
        let r = allowed
            .get(section.feature_id.as_str())
            .ok_or_else(|| AppError::Parse("执行说明引用未获支持的规则".into()))?;
        if !seen.insert(section.feature_id) || section.explanation.trim().is_empty() {
            return Err(AppError::Parse("执行说明规则重复/解释为空".into()));
        }
        let strength = if r.classification == "stable" {
            "优先采用"
        } else {
            "条件适用，非通用习惯"
        };
        md.push_str(&format!(
            "## {}\n\n适用条件：{}。{}。\n\n{}\n\n",
            r.candidate.rule, r.candidate.condition, strength, section.explanation
        ));
    }
    if seen.len() != allowed.len() {
        return Err(AppError::Parse("执行说明遗漏已选规则".into()));
    }
    if doc.evidence_insufficient {
        md.push_str("证据限制：现有材料不足以支撑更详尽的通用规则。\n");
    }
    Ok(md)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn sources() -> Vec<Source> {
        (0..5)
            .map(|i| Source {
                id: format!("a{i}"),
                article_id: format!("article{i}"),
                topic: if i < 3 { "daily" } else { "reading" }.into(),
                text: "这是一段真实原文证据。".into(),
                view: View {
                    features: vec![Feature {
                        id: "f1".into(),
                        dimension: "voice".into(),
                        rule: "克制".into(),
                        condition: "叙述".into(),
                        quote: "原始风格说明的引用".into(),
                    }],
                    uncertainties: vec![],
                },
            })
            .collect()
    }
    fn model() -> serde_json::Value {
        json!({"rules":[{"id":"p1","dimension":"voice","rule":"保持克制语气","condition":"叙述事实",
            "evidence":(0..5).map(|i|json!({"article":format!("a{i}"),"status":"support","feature_ids":["f1"],"quote":"这是一段真实原文证据。","reason":"可观察语气"})).collect::<Vec<_>>()}],"omissions":[]})
    }
    #[test]
    fn counts_distinct_articles_and_keeps_unknown_out_of_denominator() {
        let s = sources();
        let mut m = model();
        let p = parse(&m.to_string(), &s).unwrap();
        assert_eq!(p.rules[0].classification, "stable");
        m["rules"][0]["evidence"][4]["status"] = json!("unknown");
        m["rules"][0]["evidence"][4]["quote"] = json!("");
        let p = parse(&m.to_string(), &s).unwrap();
        assert_eq!(p.rules[0].applicable_n, 4);
        assert_eq!(p.rules[0].support_ratio, Some(1.0));
        assert_eq!(p.rules[0].classification, "weak");
        m["rules"][0]["evidence"][3]["status"] = json!("counter");
        assert_eq!(
            parse(&m.to_string(), &s).unwrap().rules[0].classification,
            "conflict"
        );
    }
    #[test]
    fn indexed_evidence_uses_source_text_and_rejects_out_of_range_lines() {
        let sources = sources();
        let mut value = model();
        for e in value["rules"][0]["evidence"].as_array_mut().unwrap() {
            let o = e.as_object_mut().unwrap();
            o.remove("quote");
            o.insert("line_start".into(), json!(1));
            o.insert("line_end".into(), json!(1));
        }
        let profile = parse_indexed(&value.to_string(), &sources).unwrap();
        assert_eq!(
            profile.rules[0].candidate.evidence[0].quote,
            sources[0].text
        );
        value["rules"][0]["evidence"][0]["line_end"] = json!(999);
        assert!(parse_indexed(&value.to_string(), &sources).is_err());
    }
    #[test]
    fn rejects_fabricated_quotes_duplicate_votes_and_unsupported_compilation() {
        let s = sources();
        let mut m = model();
        m["rules"][0]["evidence"][0]["quote"] = json!("伪造的原文证据内容");
        assert!(parse(&m.to_string(), &s).is_err());
        m = model();
        m["rules"][0]["evidence"][1]["article"] = json!("a0");
        assert!(parse(&m.to_string(), &s).is_err());
        let p = parse(&model().to_string(), &s).unwrap();
        assert!(compile(r#"{"sections":[{"feature_id":"missing","explanation":"编造规则"}],"evidence_insufficient":false}"#,&p).is_err());
    }
}
