//! Deterministic Spec length policy, independent of model estimates.
pub fn character_count(spec: &str) -> usize {
    spec.chars().filter(|c| !c.is_whitespace()).count()
}

pub fn conciseness_score(count: usize) -> f64 {
    match count {
        400..=600 => 5.0,
        300..=399 | 601..=700 => 4.0,
        200..=299 | 701..=850 => 3.0,
        100..=199 | 851..=1000 => 2.0,
        _ => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_unicode_scalars_and_markdown_but_not_unicode_whitespace() {
        assert_eq!(character_count("## 标题\n- 中，A😀\t\r\n\u{3000}\u{a0}"), 9);
        assert_eq!(character_count(" \n\t\u{2003}"), 0);
    }

    #[test]
    fn every_length_boundary_including_underlength_is_scored() {
        for (count, score) in [
            (0, 1.0),
            (99, 1.0),
            (100, 2.0),
            (199, 2.0),
            (200, 3.0),
            (299, 3.0),
            (300, 4.0),
            (399, 4.0),
            (400, 5.0),
            (600, 5.0),
            (601, 4.0),
            (700, 4.0),
            (701, 3.0),
            (850, 3.0),
            (851, 2.0),
            (1000, 2.0),
            (1001, 1.0),
            (usize::MAX, 1.0),
        ] {
            assert_eq!(conciseness_score(count), score, "length {count}");
        }
    }
}
