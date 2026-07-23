//! Mock test blend: fixed category weights and per-category sampling kinds
//! matching the real Anytime Test composition
//! (docs/superpowers/specs/2026-07-20-mock-test-blend-design.md).

/// Real-test composition, from the tally of three archived Anytime Tests
/// (Jan 28–30, 2020). Weights sum to 100.
pub const TARGET_WEIGHTS: [(&str, i64); 13] = [
    ("Literature & Language", 20),
    ("Geography & Exploration", 14),
    ("History & Politics", 13),
    ("Science & Nature", 11),
    ("Film, TV & Pop Culture", 10),
    ("Philosophy, Religion & Society", 6),
    ("Music & Performing Arts", 6),
    ("Miscellaneous", 6),
    ("Technology & Engineering", 4),
    ("Mathematics & Logic", 4),
    ("Art & Culture", 2),
    ("Business & Economics", 2),
    ("Sports & Games", 2),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplingKind {
    /// Weight ∝ ln(1 + answer_freq): favors canonical, frequently-recurring answers.
    Canon,
    /// Weight ∝ 6-year-half-life decay on air_date: favors current material.
    Recency,
    /// Music: seats split canon/recency (composer slot + current-artist slot).
    Split,
}

pub fn sampling_kind(category: &str) -> SamplingKind {
    match category {
        "Film, TV & Pop Culture" | "Sports & Games" => SamplingKind::Recency,
        "Music & Performing Arts" => SamplingKind::Split,
        _ => SamplingKind::Canon,
    }
}

/// (canon_seats, recency_seats); canon gets the odd seat.
pub fn split_seats(total: i64) -> (i64, i64) {
    let recency = total / 2;
    (total - recency, recency)
}

/// Normalized Anytime Test share of a meta-category (TARGET_WEIGHTS sums to
/// 100). Unknown categories get a 0.02 floor so nothing zeroes out.
pub fn test_share(category: &str) -> f64 {
    TARGET_WEIGHTS
        .iter()
        .find(|(c, _)| *c == category)
        .map(|(_, w)| *w as f64 / 100.0)
        .unwrap_or(0.02)
        .max(0.02)
}

/// Fixed weights restricted to categories that have an eligible pool.
pub fn target_weights(available: &[String]) -> Vec<(String, i64)> {
    TARGET_WEIGHTS
        .iter()
        .filter(|(c, _)| available.iter().any(|a| a == c))
        .map(|(c, w)| (c.to_string(), *w))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::mock_test::apportion;

    #[test]
    fn weights_sum_to_100() {
        assert_eq!(TARGET_WEIGHTS.iter().map(|(_, w)| w).sum::<i64>(), 100);
    }

    #[test]
    fn full_apportionment_yields_50_seats_with_lit_at_10() {
        let dist: Vec<(String, i64)> = TARGET_WEIGHTS
            .iter()
            .map(|(c, w)| (c.to_string(), *w))
            .collect();
        let q = apportion(&dist, 50);
        assert_eq!(q.iter().map(|(_, s)| s).sum::<i64>(), 50);
        let lit = q.iter().find(|(c, _)| c == "Literature & Language").unwrap().1;
        assert_eq!(lit, 10); // 20% of 50
    }

    #[test]
    fn split_seats_gives_canon_the_odd_seat() {
        assert_eq!(split_seats(0), (0, 0));
        assert_eq!(split_seats(1), (1, 0));
        assert_eq!(split_seats(3), (2, 1));
        assert_eq!(split_seats(4), (2, 2));
    }

    #[test]
    fn sampling_kinds_match_spec() {
        assert_eq!(sampling_kind("Film, TV & Pop Culture"), SamplingKind::Recency);
        assert_eq!(sampling_kind("Sports & Games"), SamplingKind::Recency);
        assert_eq!(sampling_kind("Music & Performing Arts"), SamplingKind::Split);
        assert_eq!(sampling_kind("Literature & Language"), SamplingKind::Canon);
        assert_eq!(sampling_kind("History & Politics"), SamplingKind::Canon);
    }

    #[test]
    fn test_share_normalizes_and_floors() {
        assert!((test_share("Literature & Language") - 0.20).abs() < 1e-9);
        assert!((test_share("Sports & Games") - 0.02).abs() < 1e-9);
        assert!((test_share("No Such Category") - 0.02).abs() < 1e-9);
    }

    #[test]
    fn target_weights_filters_to_available_categories() {
        let available = vec![
            "Literature & Language".to_string(),
            "Sports & Games".to_string(),
        ];
        let w = target_weights(&available);
        assert_eq!(w.len(), 2);
        assert!(w.iter().any(|(c, n)| c == "Literature & Language" && *n == 20));
        assert!(w.iter().any(|(c, n)| c == "Sports & Games" && *n == 2));
    }
}
