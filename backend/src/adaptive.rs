//! Adaptive weakness targeting: turn per-category attempt history into a
//! normalized "weakness" distribution over classifier categories.
//!
//! Pure and deterministic — no DB, no clock, and randomness is passed IN
//! (`sample_category` takes `r`), so everything here is unit-testable.

const PRIOR_PSEUDO_COUNT: f64 = 5.0;

#[derive(Debug, Clone)]
pub struct CategoryStat {
    pub category: String,
    pub attempts: i64,
    pub correct: i64,
}

#[derive(Debug, Clone)]
pub struct CategoryWeight {
    pub category: String,
    pub attempts: i64,
    pub accuracy: f64, // observed percent, 0 when unattempted
    pub weight: f64,   // normalized selection probability
}

/// Smoothed miss-rate weights, sorted by weight descending.
/// smoothed_acc = (correct + 5·global_acc) / (attempts + 5); raw = 1 − smoothed.
/// Empty input → empty. All-raw-zero (perfect everywhere) → uniform.
pub fn compute_weights(stats: &[CategoryStat]) -> Vec<CategoryWeight> {
    if stats.is_empty() {
        return vec![];
    }
    let total_attempts: i64 = stats.iter().map(|s| s.attempts).sum();
    let total_correct: i64 = stats.iter().map(|s| s.correct).sum();
    let global_acc = if total_attempts > 0 {
        total_correct as f64 / total_attempts as f64
    } else {
        0.5
    };

    let raw: Vec<f64> = stats
        .iter()
        .map(|s| {
            let smoothed = (s.correct as f64 + PRIOR_PSEUDO_COUNT * global_acc)
                / (s.attempts as f64 + PRIOR_PSEUDO_COUNT);
            (1.0 - smoothed).max(0.0)
        })
        .collect();

    let sum: f64 = raw.iter().sum();
    let n = stats.len() as f64;

    let mut out: Vec<CategoryWeight> = stats
        .iter()
        .zip(raw.iter())
        .map(|(s, &r)| CategoryWeight {
            category: s.category.clone(),
            attempts: s.attempts,
            accuracy: if s.attempts > 0 {
                s.correct as f64 / s.attempts as f64 * 100.0
            } else {
                0.0
            },
            weight: if sum > 0.0 { r / sum } else { 1.0 / n },
        })
        .collect();

    out.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Walk the cumulative distribution with r ∈ [0,1). Returns the last entry for
/// float dust at r ≈ 1.0; None only for an empty slice.
pub fn sample_category(weights: &[CategoryWeight], r: f64) -> Option<&str> {
    let mut acc = 0.0;
    for w in weights {
        acc += w.weight;
        if r < acc {
            return Some(&w.category);
        }
    }
    weights.last().map(|w| w.category.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(cat: &str, attempts: i64, correct: i64) -> CategoryStat {
        CategoryStat { category: cat.to_string(), attempts, correct }
    }

    #[test]
    fn weights_normalize_and_favor_weak_categories() {
        let stats = vec![s("Music", 100, 55), s("Math", 100, 82), s("Science", 100, 74)];
        let w = compute_weights(&stats);
        let sum: f64 = w.iter().map(|x| x.weight).sum();
        assert!((sum - 1.0).abs() < 1e-9);
        // Sorted descending by weight: weakest (Music) first, strongest (Math) last.
        assert_eq!(w[0].category, "Music");
        assert_eq!(w[2].category, "Math");
        assert!(w[0].weight > w[2].weight);
    }

    #[test]
    fn smoothing_keeps_tiny_samples_from_dominating() {
        // One miss on a single attempt must not outrank a genuinely weak,
        // well-sampled category (50% over 100 attempts vs 0% over 1).
        let stats = vec![s("Tiny", 1, 0), s("BigWeak", 100, 50), s("Strong", 100, 90)];
        let w = compute_weights(&stats);
        let get = |c: &str| w.iter().find(|x| x.category == c).unwrap().weight;
        assert!(get("BigWeak") > get("Tiny"));
        assert!(get("Tiny") > get("Strong"));
    }

    #[test]
    fn empty_input_gives_empty_output() {
        assert!(compute_weights(&[]).is_empty());
    }

    #[test]
    fn all_perfect_falls_back_to_uniform() {
        // global_acc = 1.0 → every raw weight is 0 → uniform distribution.
        let stats = vec![s("A", 10, 10), s("B", 10, 10)];
        let w = compute_weights(&stats);
        assert!((w[0].weight - 0.5).abs() < 1e-9);
        assert!((w[1].weight - 0.5).abs() < 1e-9);
    }

    #[test]
    fn accuracy_is_observed_percent() {
        let stats = vec![s("A", 100, 55), s("Zero", 0, 0)];
        let w = compute_weights(&stats);
        let get = |c: &str| w.iter().find(|x| x.category == c).unwrap();
        assert!((get("A").accuracy - 55.0).abs() < 1e-9);
        assert!((get("Zero").accuracy - 0.0).abs() < 1e-9);
    }

    #[test]
    fn sampling_walks_the_cumulative_distribution() {
        let stats = vec![s("Weak", 100, 20), s("Strong", 100, 90)];
        let w = compute_weights(&stats);
        // w[0] = Weak with the larger weight.
        assert_eq!(sample_category(&w, 0.0), Some("Weak"));
        assert_eq!(sample_category(&w, 0.999_999), Some("Strong"));
        assert_eq!(sample_category(&w, w[0].weight + 1e-9), Some("Strong"));
        assert_eq!(sample_category(&[], 0.5), None);
    }
}
