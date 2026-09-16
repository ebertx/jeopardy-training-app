#![allow(dead_code)]

//! Hooks: an entity's clue angles, mined from its own clues (spec §2), plus
//! the pure drill helpers (spec §3). No DB here — `objects.rs` feeds it.

use std::collections::{BTreeMap, BTreeSet};

pub const HOOK_MIN_SUPPORT: i64 = 2;
pub const HOOK_MAX_PER_ENTITY: usize = 8;
pub const HOOK_UNIGRAM_MAX_DF: i64 = 3000;
pub const HOOK_BIGRAM_MAX_DF: i64 = 300;
pub const HOOK_MERGE_JACCARD: f64 = 0.5;
/// Only the most distinctive grams take part in clustering; hooks are capped
/// at HOOK_MAX_PER_ENTITY anyway, so the top seeds by support × idf carry every
/// angle that could rank. Bounds the pairwise merge loop on huge entities.
pub const HOOK_MAX_SEEDS: usize = 60;

#[derive(Debug, Clone, PartialEq)]
pub struct GramStat {
    pub gram: String,
    pub n: i16,
    pub support: i64,
    pub corpus_df: i64,
    pub clue_ids: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cluster {
    /// The cluster's most distinctive gram (max support × idf); stable identity.
    pub key_gram: String,
    pub grams: Vec<String>,
    pub clue_ids: Vec<i32>,
    pub support: usize,
}

/// ln(corpus / df), floored at 0. Zero-safe.
pub fn idf(corpus_clues: i64, df: i64) -> f64 {
    (corpus_clues.max(1) as f64 / df.max(1) as f64).ln().max(0.0)
}

/// A gram seeds a cluster when it recurs within the entity and is not a
/// corpus-wide stop-like term.
pub fn is_seed(g: &GramStat) -> bool {
    let cap = if g.n == 2 { HOOK_BIGRAM_MAX_DF } else { HOOK_UNIGRAM_MAX_DF };
    g.support >= HOOK_MIN_SUPPORT && g.corpus_df <= cap
}

/// Containment counts as 1.0; otherwise Jaccard when it clears the bar.
fn merge_score(a: &BTreeSet<i32>, b: &BTreeSet<i32>) -> Option<f64> {
    if a.is_empty() || b.is_empty() {
        return None;
    }
    if a.is_subset(b) || b.is_subset(a) {
        return Some(1.0);
    }
    let inter = a.intersection(b).count();
    if inter == 0 {
        return None;
    }
    let j = inter as f64 / (a.len() + b.len() - inter) as f64;
    (j >= HOOK_MERGE_JACCARD).then_some(j)
}

struct Work {
    grams: Vec<usize>, // indices into `seeds`
    clues: BTreeSet<i32>,
}

/// Cluster an entity's grams into hooks:
/// 1. seeds = grams passing `is_seed`, the top HOOK_MAX_SEEDS by support × idf, ordered by support desc, gram asc;
/// 2. greedy agglomeration — repeatedly merge the pair with the highest
///    `merge_score` (first pair wins ties) until none qualifies;
/// 3. every clue is assigned to the one cluster whose grams present in the
///    clue carry the most idf weight (ties: the larger cluster, then earlier);
/// 4. clusters below HOOK_MIN_SUPPORT are dropped; the rest are ranked by
///    support desc, key_gram asc, and capped at HOOK_MAX_PER_ENTITY.
pub fn cluster_hooks(grams: &[GramStat], corpus_clues: i64) -> Vec<Cluster> {
    let mut seeds: Vec<&GramStat> = grams.iter().filter(|g| is_seed(g)).collect();
    // Keep the HOOK_MAX_SEEDS most distinctive grams (support × idf), ties by gram.
    seeds.sort_by(|a, b| {
        let wa = a.support as f64 * idf(corpus_clues, a.corpus_df);
        let wb = b.support as f64 * idf(corpus_clues, b.corpus_df);
        wb.partial_cmp(&wa).unwrap_or(std::cmp::Ordering::Equal).then(a.gram.cmp(&b.gram))
    });
    seeds.truncate(HOOK_MAX_SEEDS);
    seeds.sort_by(|a, b| b.support.cmp(&a.support).then(a.gram.cmp(&b.gram)));
    let weight: Vec<f64> = seeds.iter().map(|g| idf(corpus_clues, g.corpus_df)).collect();

    let mut work: Vec<Work> = seeds
        .iter()
        .enumerate()
        .map(|(i, g)| Work { grams: vec![i], clues: g.clue_ids.iter().copied().collect() })
        .collect();

    loop {
        let mut best: Option<(usize, usize, f64)> = None;
        for i in 0..work.len() {
            for j in (i + 1)..work.len() {
                if let Some(s) = merge_score(&work[i].clues, &work[j].clues) {
                    if best.map_or(true, |(_, _, bs)| s > bs) {
                        best = Some((i, j, s));
                    }
                }
            }
        }
        let Some((i, j, _)) = best else { break };
        let Work { grams: gj, clues: cj } = work.remove(j);
        work[i].grams.extend(gj);
        work[i].clues.extend(cj);
    }

    // clue -> seed indices whose clue set contains it
    let mut clue_grams: BTreeMap<i32, Vec<usize>> = BTreeMap::new();
    for (gi, g) in seeds.iter().enumerate() {
        for c in &g.clue_ids {
            clue_grams.entry(*c).or_default().push(gi);
        }
    }
    let mut assigned: Vec<BTreeSet<i32>> = vec![BTreeSet::new(); work.len()];
    for (clue, gis) in &clue_grams {
        let mut best: Option<(usize, f64, usize)> = None;
        for (wi, w) in work.iter().enumerate() {
            let score: f64 = gis.iter().filter(|gi| w.grams.contains(gi)).map(|gi| weight[*gi]).sum();
            if score <= 0.0 {
                continue;
            }
            let size = w.clues.len();
            let better = match best {
                None => true,
                Some((_, bs, bsize)) => score > bs || (score == bs && size > bsize),
            };
            if better {
                best = Some((wi, score, size));
            }
        }
        if let Some((wi, _, _)) = best {
            assigned[wi].insert(*clue);
        }
    }

    let mut out: Vec<Cluster> = work
        .iter()
        .zip(assigned)
        .filter_map(|(w, clues)| {
            if (clues.len() as i64) < HOOK_MIN_SUPPORT {
                return None;
            }
            let key = w.grams.iter().copied().max_by(|&a, &b| {
                let sa = seeds[a].support as f64 * weight[a];
                let sb = seeds[b].support as f64 * weight[b];
                sa.partial_cmp(&sb)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(seeds[b].gram.cmp(&seeds[a].gram)) // tie: alphabetically first
            })?;
            let mut gs: Vec<String> = w.grams.iter().map(|&g| seeds[g].gram.clone()).collect();
            gs.sort();
            Some(Cluster {
                key_gram: seeds[key].gram.clone(),
                grams: gs,
                support: clues.len(),
                clue_ids: clues.into_iter().collect(),
            })
        })
        .collect();
    out.sort_by(|a, b| b.support.cmp(&a.support).then(a.key_gram.cmp(&b.key_gram)));
    out.truncate(HOOK_MAX_PER_ENTITY);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(gram: &str, n: i16, corpus_df: i64, clues: &[i32]) -> GramStat {
        GramStat { gram: gram.into(), n, support: clues.len() as i64, corpus_df, clue_ids: clues.to_vec() }
    }

    fn visigoths() -> Vec<GramStat> {
        vec![
            g("alar", 1, 18, &[192216, 201593, 246946, 252909, 296899, 421463, 503952, 528821]),
            g("sack", 1, 209, &[201593, 241725, 252909, 296899, 421463, 503952, 528821]),
            g("rome", 1, 1070, &[201593, 241725, 252909, 296899, 421463, 503952, 528821]),
            g("sack rome", 2, 23, &[201593, 241725, 252909, 503952]),
            g("410", 1, 15, &[252909, 421463, 503952]),
            g("ostrogoth", 1, 9, &[31327, 44239, 105561, 186092, 411040, 479520, 507846]),
            g("goth split", 2, 5, &[31327, 44239, 105561]),
            g("western goth", 2, 2, &[105561, 186092]),
            g("spain", 1, 1254, &[186092, 241725, 249894, 490093, 496405]),
            g("711", 1, 15, &[186092, 249894, 496405]),
            g("peopl", 1, 6484, &[31327, 201593, 241725, 249894, 411040, 490093, 496405, 507846]),
            g("king", 1, 7762, &[192216, 246946, 249894, 490093, 496405]),
        ]
    }

    #[test]
    fn idf_is_log_ratio_floored_at_zero() {
        assert!((idf(530_000, 18) - 10.29).abs() < 0.01);
        assert_eq!(idf(100, 1000), 0.0);
        assert_eq!(idf(0, 0), 0.0);
    }

    #[test]
    fn seeds_need_support_and_distinctiveness() {
        assert!(is_seed(&g("spain", 1, 1254, &[1, 2])));
        assert!(!is_seed(&g("peopl", 1, 6484, &[1, 2, 3])));
        assert!(!is_seed(&g("4th centuri", 2, 301, &[1, 2])));
        assert!(is_seed(&g("4th centuri", 2, 300, &[1, 2])));
        assert!(!is_seed(&g("toledo", 1, 40, &[1])));
    }

    #[test]
    fn visigoths_cluster_into_three_angles_in_support_order() {
        let out = cluster_hooks(&visigoths(), 530_000);
        let keys: Vec<&str> = out.iter().map(|c| c.key_gram.as_str()).collect();
        assert_eq!(keys, vec!["alar", "ostrogoth", "711"]);
        assert_eq!(out.iter().map(|c| c.support).collect::<Vec<_>>(), vec![9, 7, 3]);

        assert_eq!(out[0].grams, vec!["410", "alar", "rome", "sack", "sack rome"]);
        assert!(out[0].clue_ids.contains(&241725), "sack-of-Rome clue that also mentions Spain goes to Alaric");
        assert!(out[0].clue_ids.contains(&192216), "Alaric II clue goes to Alaric");

        assert_eq!(out[1].grams, vec!["goth split", "ostrogoth", "western goth"]);
        assert!(out[1].clue_ids.contains(&186092), "western-Goths-ruled-Spain clue weighs Ostrogoth grams higher");

        assert_eq!(out[2].grams, vec!["711", "spain"]);
        assert_eq!(out[2].clue_ids, vec![249894, 490093, 496405]);
    }

    #[test]
    fn clue_lists_are_sorted_and_disjoint() {
        let out = cluster_hooks(&visigoths(), 530_000);
        let mut all: Vec<i32> = out.iter().flat_map(|c| c.clue_ids.iter().copied()).collect();
        let n = all.len();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), n);
        for c in &out {
            let mut s = c.clue_ids.clone();
            s.sort();
            assert_eq!(s, c.clue_ids);
        }
    }

    #[test]
    fn clusters_are_capped_at_eight() {
        let grams: Vec<GramStat> = (0..12)
            .map(|i| g(&format!("g{i:02}"), 1, 10, &[i * 10, i * 10 + 1]))
            .collect();
        let out = cluster_hooks(&grams, 530_000);
        assert_eq!(out.len(), HOOK_MAX_PER_ENTITY);
        assert_eq!(out[0].key_gram, "g00"); // equal support: key_gram ascending
    }

    #[test]
    fn a_cluster_whose_clues_are_claimed_elsewhere_is_dropped() {
        // B = {4,5} does not merge with A = {1,2,3,4} (Jaccard 1/5); clue 4 is
        // assigned to A (higher idf) so B keeps only clue 5 and falls below
        // HOOK_MIN_SUPPORT.
        let out = cluster_hooks(&[g("a", 1, 10, &[1, 2, 3, 4]), g("b", 1, 2000, &[4, 5])], 530_000);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].key_gram, "a");
        assert_eq!(out[0].clue_ids, vec![1, 2, 3, 4]);
    }

    #[test]
    fn identical_clue_sets_merge_and_containment_merges() {
        let out = cluster_hooks(
            &[g("x", 1, 100, &[1, 2, 3]), g("x y", 2, 50, &[1, 2, 3]), g("z", 1, 100, &[2, 3])],
            530_000,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].grams, vec!["x", "x y", "z"]);
        assert_eq!(out[0].key_gram, "x y"); // 3 × idf(50) beats 3 × idf(100)
    }

    #[test]
    fn seeds_are_capped_by_weight_before_clustering() {
        // 60 distinctive seeds (df 10, 2 clues each) plus one "zz" seed that has the
        // highest support (3) but the lowest support × idf (df 2999). Without the cap
        // "zz" would lead the support-ordered seed list; with it, "zz" is cut.
        let mut grams: Vec<GramStat> = (0..60)
            .map(|i| g(&format!("g{i:02}"), 1, 10, &[i * 10, i * 10 + 1]))
            .collect();
        grams.push(g("zz", 1, 2999, &[9001, 9002, 9003]));
        let out = cluster_hooks(&grams, 530_000);
        assert_eq!(out.len(), HOOK_MAX_PER_ENTITY);
        assert!(out.iter().all(|c| c.key_gram != "zz"), "lowest-weight seed must be cut by HOOK_MAX_SEEDS");
        assert_eq!(out[0].key_gram, "g00");
    }

    #[test]
    fn empty_input_yields_no_hooks() {
        assert!(cluster_hooks(&[], 530_000).is_empty());
    }
}
