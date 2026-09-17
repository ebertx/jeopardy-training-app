//! Hooks: an entity's clue angles, mined from its own clues (spec §2), plus
//! the pure drill helpers (spec §3). No DB here — `objects.rs` feeds it.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::pavlov::{norm_tokens, phrase_leaks_answer, trim_scaffolding};

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

// ---------------------------------------------------------------- drill helpers (spec §3)

#[derive(Debug, Clone, PartialEq)]
pub struct HookExposure {
    pub id: i32,
    pub rank: i32,
    pub seen: i64,
    pub last_wrong: bool,
}

/// Fewest exposures → last rating wrong → rank → id.
pub fn pick_hook(hooks: &[HookExposure]) -> Option<i32> {
    hooks
        .iter()
        .min_by(|a, b| {
            a.seen
                .cmp(&b.seen)
                .then(b.last_wrong.cmp(&a.last_wrong)) // true (wrong) sorts first
                .then(a.rank.cmp(&b.rank))
                .then(a.id.cmp(&b.id))
        })
        .map(|h| h.id)
}

pub const HOOK_COVERAGE_CAP_DAYS: f64 = 7.0;
const DAY_SECS: i64 = 86_400;

/// While the entity still has a labeled hook this user has never seen, no
/// rating may schedule it further out than HOOK_COVERAGE_CAP_DAYS.
pub fn cap_for_coverage(interval_days: f64, interval_secs: i64, has_unseen: bool) -> (f64, i64) {
    if has_unseen && interval_days > HOOK_COVERAGE_CAP_DAYS {
        (HOOK_COVERAGE_CAP_DAYS, HOOK_COVERAGE_CAP_DAYS as i64 * DAY_SECS)
    } else {
        (interval_days, interval_secs)
    }
}

// ---------------------------------------------------------------- vetted import (spec §2)

#[derive(Debug, Clone, PartialEq)]
pub struct VettedPair {
    pub cue: String,
    pub response: String,
}

/// The JBoard lists join several pavlovs for one response with " & "
/// ("hudson river & hudson bay & killed in mutiny"). Each piece is its own
/// cue. Pieces that are scrape noise are dropped: all-caps nationality
/// shorthand ("NORWAY"), unbalanced quotes (a line clipped mid-title), forum
/// prose longer than VETTED_MAX_CUE_CHARS, empties.
pub const VETTED_MAX_CUE_CHARS: usize = 80;

pub fn vetted_cue_pieces(cue: &str) -> Vec<String> {
    cue.split(" & ")
        .map(|p| p.trim().trim_matches(|c: char| c == ',' || c == ';').trim().to_string())
        .filter(|p| !p.is_empty())
        .filter(|p| p.chars().count() <= VETTED_MAX_CUE_CHARS)
        .filter(|p| p.matches('"').count() % 2 == 0)
        .filter(|p| !(p.chars().all(|c| c.is_uppercase() || !c.is_alphabetic()) && p.chars().any(|c| c.is_alphabetic())))
        .collect()
}

/// `cue\tresponse\tdomain\tsource_url` rows; header and malformed lines
/// skipped; "&"-joined cues expanded into one pair per piece.
pub fn parse_vetted_tsv(s: &str) -> Vec<VettedPair> {
    s.lines()
        .skip(1)
        .flat_map(|line| {
            let mut it = line.split('\t');
            let cue = it.next().unwrap_or("").trim();
            let response = it.next().unwrap_or("").trim().to_string();
            if cue.is_empty() || response.is_empty() {
                return vec![];
            }
            vetted_cue_pieces(cue)
                .into_iter()
                .map(|piece| VettedPair { cue: piece, response: response.clone() })
                .collect()
        })
        .collect()
}

/// A vetted cue (as Postgres `english` lexemes) labels a cluster when every
/// token of at least one cluster gram is among the lexemes.
pub fn vetted_matches(cue_lexemes: &[String], grams: &[String]) -> bool {
    grams.iter().any(|g| {
        let toks: Vec<&str> = g.split_whitespace().collect();
        !toks.is_empty() && toks.iter().all(|t| cue_lexemes.iter().any(|l| l == t))
    })
}

// ---------------------------------------------------------------- model labels (spec §2)

pub const HOOK_LABEL_MODEL: &str = "gpt-4o-mini";
pub const HOOK_LABEL_BATCH: i64 = 20;
/// Label batches in flight at once; each is claimed before its call.
pub const HOOK_LABEL_PARALLEL: usize = 4;
const HEDGES: &[&str] = &["possibly", "perhaps", "often", "maybe", "sometimes"];
const MAX_LABEL_WORDS: usize = 8;

#[derive(Debug, Clone)]
pub struct HookLabelInput {
    pub answer: String,
    pub key_gram: String,
    pub grams: Vec<String>,
    pub sample_clues: Vec<String>,
    /// An existing cue-source label to tidy (a phrase clipped from one clue);
    /// `None` for a fresh label.
    pub hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HookLabelOutcome {
    pub answer: String,
    pub key_gram: String,
    pub cue: Option<String>,
}

/// No keep/discard flag: gpt-4o-mini flips such a flag for whole batches at a
/// time (197 of 801 batches came back all-false with good cues on the first
/// live run). The grounding and support gates already reject nonsense.
pub fn hook_label_prompts(batch: &[HookLabelInput]) -> (String, String) {
    let system = "You write short Jeopardy! cue labels. For each item you get an answer, the stemmed key \
terms of ONE angle the clue writers use for that answer, real clues from that angle, and sometimes a \
draft cue clipped from one clue. Return a cue of 2 to 8 words naming that angle the way a Jeopardy! \
clue would, built only from words that appear in the supplied clues or draft (names, dates, places, \
works). When a draft is given, return it unchanged if it already reads as a complete phrase; otherwise \
rewrite it into one — no dangling connectives, no trailing commas, no leading 'In 1804 he'. NEVER \
include the answer or any word of the answer. No hedges (possibly, perhaps, often). No leading 'this' \
or 'these'. Respond with JSON only: {\"results\": [{\"answer\": string (echoed verbatim), \
\"key_gram\": string (echoed verbatim), \"cue\": string}]}"
        .to_string();
    let items: Vec<Value> = batch
        .iter()
        .map(|b| {
            let mut item = serde_json::json!({
                "answer": b.answer,
                "key_gram": b.key_gram,
                "terms": b.grams,
                "clues": b.sample_clues,
            });
            if let Some(h) = &b.hint {
                item["draft"] = Value::String(h.clone());
            }
            item
        })
        .collect();
    let user = serde_json::to_string_pretty(&serde_json::json!({ "hooks": items })).expect("serializable");
    (system, user)
}

/// Every content token (≥ 4 chars) of `cue` appears as a token of some clue.
pub fn label_grounded(cue: &str, clues: &[String]) -> bool {
    let cue_toks = norm_tokens(cue);
    if cue_toks.is_empty() {
        return false;
    }
    let clue_toks: std::collections::HashSet<String> = clues.iter().flat_map(|c| norm_tokens(c)).collect();
    cue_toks.iter().filter(|t| t.chars().count() >= 4).all(|t| clue_toks.contains(t))
}

/// Lenient parse; every gate from the spec applied (length, answer leak,
/// grounding in the sample clues or the draft, hedges). Items with no
/// matching input are skipped; an item that fails a gate yields `cue: None`.
pub fn parse_hook_labels(v: &Value, inputs: &[HookLabelInput]) -> Vec<HookLabelOutcome> {
    let Some(results) = v.get("results").and_then(|r| r.as_array()) else {
        return vec![];
    };
    results
        .iter()
        .filter_map(|item| {
            let answer = item.get("answer")?.as_str()?.trim().to_string();
            let key_gram = item.get("key_gram")?.as_str()?.trim().to_string();
            let input = inputs
                .iter()
                .find(|i| i.answer.eq_ignore_ascii_case(&answer) && i.key_gram == key_gram)?;
            let raw = item
                .get("cue")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .trim()
                .trim_matches(['"', '\'', '\u{201C}', '\u{201D}', '\u{2018}', '\u{2019}'])
                .to_string();
            let cue = trim_scaffolding(&raw);
            let words = cue.split_whitespace().count();
            let hedged = norm_tokens(&cue).iter().any(|t| HEDGES.contains(&t.as_str()));
            let mut grounding = input.sample_clues.clone();
            grounding.extend(input.hint.iter().cloned());
            let ok = !cue.is_empty()
                && words <= MAX_LABEL_WORDS
                && !phrase_leaks_answer(&input.answer, &cue)
                && label_grounded(&cue, &grounding)
                && !hedged;
            Some(HookLabelOutcome {
                answer: input.answer.clone(),
                key_gram,
                cue: ok.then_some(cue),
            })
        })
        .collect()
}

// ---------------------------------------------------------------- hook hygiene

/// Key-gram stems (Postgres `english`) that name what an entity *is* rather
/// than an angle on it: professions, nationalities, category nouns. A cluster
/// keyed by one of these ("english playwright", "type of reptile") is the
/// identity frame, not a pavlov. One such hook per entity is kept — it is
/// often the highest-frequency angle — the rest are dropped after mining.
pub const FRAME_GRAMS: &[&str] = &[
    "actor", "actress", "african", "album", "american", "ancient", "anim", "apostl", "arab", "architect",
    "argentin", "artist", "asian", "australian", "austrian", "author", "band", "beer", "belgian", "biblic",
    "bird", "bone", "brand", "brazilian", "british", "call", "canadian", "capit", "car", "charact", "chees",
    "chemist", "chines", "citi", "classic", "coloni", "comedian", "compani", "compos", "conductor", "contin",
    "countri", "czech", "dancer", "danish", "desert", "dictat", "director", "discipl", "diseas", "dish",
    "dramatist", "drink", "dutch", "egyptian", "element", "emperor", "empir", "english", "european", "explor",
    "famous", "film", "finnish", "fish", "flower", "food", "french", "fruit", "game", "gas", "general",
    "german", "god", "goddess", "governor", "greek", "group", "hero", "heroin", "hungarian", "indian",
    "insect", "instrument", "inventor", "irish", "island", "isra", "italian", "japanes", "king", "kingdom",
    "known", "lake", "latin", "leader", "legendari", "liquor", "mammal", "mayor", "metal", "mexican", "miner",
    "modern", "mountain", "movi", "musician", "mytholog", "name", "nation", "norwegian", "novel", "novelist",
    "ocean", "opera", "organ", "painter", "peninsula", "persian", "philosoph", "physicist", "pianist",
    "plant", "play", "playwright", "poem", "poet", "polish", "pope", "portugues", "presid", "product",
    "prophet", "provinc", "queen", "region", "reptil", "republ", "river", "roman", "russian", "saint", "sauc",
    "scientist", "scottish", "sculptor", "sea", "senat", "singer", "sitcom", "song", "spanish", "spirit",
    "sport", "state", "statesman", "swedish", "swiss", "tree", "turkish", "veget", "villain", "violinist",
    "welsh", "wine", "writer",
];

/// Words that never make two cues the same angle: frames again, plus the
/// connective vocabulary of clue writing. Everything here is ≥ 5 chars, the
/// shortest token `cue_content_tokens` keeps.
const MERGE_STOP: &[&str] = &[
    "about", "after", "album", "albums", "american", "ancient", "animal", "artist", "author", "battle",
    "before", "between", "black", "books", "british", "called", "capital", "century", "character", "chinese",
    "cities", "classic", "classical", "composer", "country", "during", "early", "emperor", "empire", "english",
    "european", "famous", "film", "films", "first", "founded", "founder", "french", "general", "german", "great",
    "greek", "group", "highest", "indian", "irish", "island", "islands", "italian", "japanese", "kingdom", "known",
    "largest", "later", "leader", "little", "longest", "modern", "mountain", "mountains", "movie", "movies",
    "named", "nation", "north", "novel", "novels", "ocean", "oldest", "opera", "painted", "painter", "people",
    "planet", "player", "players", "plays", "poems", "president", "queen", "republic", "river", "roman", "russian",
    "series", "singer", "smallest", "south", "spanish", "species", "sport", "sports", "state", "states", "story",
    "stories", "their", "there", "these", "title", "under", "united", "where", "which", "white", "woman", "women",
    "works", "world", "writer", "written", "wrote", "years",
    // professions and nationalities as whole words
    "actor", "actress", "african", "arabic", "architect", "asian", "austrian", "belgian", "brazilian", "canadian",
    "chemist", "comedian", "conductor", "dancer", "danish", "dictator", "director", "dramatist", "dutch", "egyptian",
    "explorer", "finnish", "goddess", "hungarian", "inventor", "mexican", "musician", "norwegian", "novelist",
    "persian", "philosopher", "physicist", "pianist", "playwright", "polish", "portuguese", "prophet", "saint",
    "scientist", "scottish", "sculptor", "statesman", "swedish", "swiss", "turkish", "villain", "welsh",
];

/// Lowercased alphanumeric tokens of ≥ 5 chars, a trailing plural "s"
/// folded, stop words removed. Two cues sharing one are the same angle.
pub fn cue_content_tokens(cue: &str) -> BTreeSet<String> {
    cue.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 5 && !w.chars().all(|c| c.is_ascii_digit()))
        .map(|w| match w.strip_suffix('s') {
            Some(stem) if stem.chars().count() >= 4 && !w.ends_with("ss") => stem.to_string(),
            _ => w.to_string(),
        })
        .filter(|w| !MERGE_STOP.contains(&w.as_str()) && !MERGE_STOP.contains(&format!("{w}s").as_str()))
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct HookCue {
    pub id: i32,
    pub cue: String,
    pub support: i64,
    /// vetted / both — a human label wins the merge regardless of support.
    pub vetted: bool,
}

/// `(keep, drop)` pairs among one entity's labeled hooks: hooks are visited
/// in priority order (vetted first, then support, then id); each later hook
/// that shares a content token with an already-kept hook merges into it.
pub fn duplicate_hook_pairs(hooks: &[HookCue]) -> Vec<(i32, i32)> {
    let mut ordered: Vec<&HookCue> = hooks.iter().collect();
    ordered.sort_by(|a, b| b.vetted.cmp(&a.vetted).then(b.support.cmp(&a.support)).then(a.id.cmp(&b.id)));
    let mut kept: Vec<(i32, BTreeSet<String>)> = Vec::new();
    let mut pairs = Vec::new();
    for h in ordered {
        let toks = cue_content_tokens(&h.cue);
        match kept.iter().find(|(_, kt)| !kt.is_disjoint(&toks)) {
            Some((keep, _)) => pairs.push((*keep, h.id)),
            None => kept.push((h.id, toks)),
        }
    }
    pairs
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

    fn h(id: i32, rank: i32, seen: i64, last_wrong: bool) -> HookExposure {
        HookExposure { id, rank, seen, last_wrong }
    }

    #[test]
    fn pick_hook_prefers_unseen_then_last_wrong_then_rank() {
        assert_eq!(pick_hook(&[h(1, 1, 3, false), h(2, 2, 0, false), h(3, 3, 0, false)]), Some(2));
        assert_eq!(pick_hook(&[h(1, 1, 2, false), h(2, 2, 2, true), h(3, 3, 2, false)]), Some(2));
        assert_eq!(pick_hook(&[h(1, 2, 1, false), h(2, 1, 1, false)]), Some(2));
        assert_eq!(pick_hook(&[]), None);
    }

    #[test]
    fn coverage_cap_only_bites_with_unseen_hooks_and_long_intervals() {
        assert_eq!(cap_for_coverage(30.0, 30 * 86_400, true), (7.0, 7 * 86_400));
        assert_eq!(cap_for_coverage(30.0, 30 * 86_400, false), (30.0, 30 * 86_400));
        assert_eq!(cap_for_coverage(3.0, 3 * 86_400, true), (3.0, 3 * 86_400));
        assert_eq!(cap_for_coverage(0.0, 600, true), (0.0, 600));
    }

    fn s(v: &[&str]) -> Vec<String> { v.iter().map(|x| x.to_string()).collect() }

    #[test]
    fn vetted_matches_when_every_token_of_some_gram_is_a_lexeme() {
        assert!(vetted_matches(&s(&["finnish", "compos"]), &s(&["finnish compos", "finlandia"])));
        assert!(vetted_matches(&s(&["lullabi"]), &s(&["lullabi", "requiem"])));
        assert!(!vetted_matches(&s(&["german", "requiem"]), &s(&["lullabi"])));
        assert!(!vetted_matches(&s(&["sack"]), &s(&["sack rome"])));
        assert!(!vetted_matches(&s(&[]), &s(&["x"])));
    }

    fn input(answer: &str, key: &str, grams: &[&str], clues: &[&str]) -> HookLabelInput {
        HookLabelInput { answer: answer.into(), key_gram: key.into(), grams: s(grams), sample_clues: s(clues), hint: None }
    }

    #[test]
    fn label_prompts_carry_answer_grams_and_clues_and_demand_json() {
        let (system, user) = hook_label_prompts(&[input(
            "the Visigoths", "711", &["711", "spain"],
            &["In 711 a Muslim army defeated Roderick, the last king of these people in Spain"],
        )]);
        assert!(system.contains("JSON"));
        assert!(system.to_lowercase().contains("never"));
        assert!(!system.contains("keep"), "no keep flag: the model flips it for whole batches");
        assert!(user.contains("Visigoths"));
        assert!(user.contains("\"711\""));
        assert!(user.contains("Roderick"));
        assert!(!user.contains("draft"));
    }

    #[test]
    fn label_prompts_carry_the_draft_for_tidy_items() {
        let mut i = input("Jane Austen", "sens", &["sens"], &["Marianne Dashwood is a heroine of this 1811 novel"]);
        i.hint = Some("of \"Sense & Sensibility\" who".into());
        let (_, user) = hook_label_prompts(&[i]);
        assert!(user.contains("\"draft\": \"of \\\"Sense & Sensibility\\\" who\""));
    }

    #[test]
    fn label_grounded_requires_content_words_to_appear_in_a_clue() {
        let clues = s(&["In 711 a Muslim army defeated Roderick, the last king of these people in Spain"]);
        assert!(label_grounded("last king of these people in Spain", &clues));
        assert!(label_grounded("Roderick's people", &clues));
        assert!(label_grounded("Spain in 711", &clues)); // "711" is short: not checked
        assert!(!label_grounded("kings of Toledo", &clues));
        assert!(!label_grounded("ruled Spain until 711", &clues)); // "ruled"/"until" absent
        assert!(!label_grounded("", &clues));
    }

    #[test]
    fn parse_labels_applies_every_gate() {
        let inputs = vec![
            input("the Visigoths", "711", &["711", "spain"], &["These western Goths (as opposed to the eastern Ostrogoths) ruled Spain until 711"]),
            input("the Visigoths", "alar", &["alar", "sack rome"], &["410 A.D.: Under Alaric, these \"Westerners\" sack Rome"]),
            input("Brahms", "lullabi", &["lullabi"], &["This composer of a famous Lullaby"]),
            input("Solomon", "wise", &["wise"], &["This wise king judged between two mothers"]),
            input("Solomon", "templ", &["templ"], &["He built the first temple in Jerusalem"]),
            input("Solomon", "mother", &["mother"], &["This wise king judged between two mothers"]),
        ];
        let v = serde_json::json!({ "results": [
            { "answer": "the Visigoths", "key_gram": "711", "keep": true, "cue": "\"ruled Spain until 711\"" },
            { "answer": "the Visigoths", "key_gram": "alar", "keep": true, "cue": "the Visigoths' Alaric sacks Rome" },  // leaks the answer
            { "answer": "Brahms", "key_gram": "lullabi", "keep": true, "cue": "Lullaby composer of Hamburg" },          // Hamburg not in a clue
            { "answer": "Solomon", "key_gram": "wise", "keep": false, "cue": "wise king" },                              // keep is ignored
            { "answer": "Solomon", "key_gram": "templ", "keep": true, "cue": "possibly built the first temple" },        // hedge
            { "answer": "Solomon", "key_gram": "mother", "keep": true, "cue": "this wise king who judged between two mothers and more" }, // > 8 words after trim
            { "answer": "Nobody", "key_gram": "x", "keep": true, "cue": "orphan" },                                      // no input: skipped
        ]});
        let out = parse_hook_labels(&v, &inputs);
        assert_eq!(out.len(), 6);
        assert_eq!(out[0].cue.as_deref(), Some("ruled Spain until 711"));
        assert!(out[1].cue.is_none(), "answer leak");
        assert!(out[2].cue.is_none(), "ungrounded word");
        assert_eq!(out[3].cue.as_deref(), Some("wise king"), "a stray keep=false no longer discards a good cue");
        assert!(out[4].cue.is_none(), "hedge");
        assert!(out[5].cue.is_none(), "too long");
    }

    #[test]
    fn tidy_labels_are_grounded_in_the_draft_as_well_as_the_clues() {
        let mut i = input("Jane Austen", "sens", &["sens"], &["Marianne Dashwood is a heroine of this 1811 novel"]);
        i.hint = Some("of \"Sense & Sensibility\" who".into());
        let v = serde_json::json!({ "results": [
            { "answer": "Jane Austen", "key_gram": "sens", "cue": "\"Sense & Sensibility\" novelist" },
        ]});
        let out = parse_hook_labels(&v, &[i.clone()]);
        assert!(out[0].cue.is_none(), "novelist appears in neither clue nor draft");
        let v = serde_json::json!({ "results": [
            { "answer": "Jane Austen", "key_gram": "sens", "cue": "Sense & Sensibility, Marianne Dashwood" },
        ]});
        assert_eq!(parse_hook_labels(&v, &[i]).remove(0).cue.as_deref(), Some("Sense & Sensibility, Marianne Dashwood"));
    }

    #[test]
    fn parse_labels_of_garbage_is_empty() {
        assert!(parse_hook_labels(&serde_json::json!({"nope": 1}), &[]).is_empty());
    }

    #[test]
    fn vetted_tsv_parses_rows_and_skips_header_blanks_and_shorthand() {
        let tsv = "cue\tresponse\tdomain\tsource_url\nFINLAND\tjean \"finlandia\" sibelius\trussian\thttps://x\n\nbad line\n\"lullaby\"\tjohannes brahms\tgerman\thttps://x\n";
        let rows = parse_vetted_tsv(tsv);
        assert_eq!(rows.len(), 1, "FINLAND is nationality shorthand, not a cue");
        assert_eq!(rows[0].cue, "\"lullaby\"");
        assert_eq!(rows[0].response, "johannes brahms");
    }

    #[test]
    fn vetted_tsv_expands_ampersand_joined_lines_into_one_pair_per_piece() {
        let tsv = "cue\tresponse\tdomain\tsource_url\nhudson river & hudson bay & killed in mutiny while seeking northwest passage\thenry hudson\texplorers\thttps://x\n";
        let rows = parse_vetted_tsv(tsv);
        let cues: Vec<&str> = rows.iter().map(|r| r.cue.as_str()).collect();
        assert_eq!(cues, vec!["hudson river", "hudson bay", "killed in mutiny while seeking northwest passage"]);
        assert!(rows.iter().all(|r| r.response == "henry hudson"));
    }

    #[test]
    fn vetted_pieces_drop_scrape_noise() {
        assert_eq!(vetted_cue_pieces("cafe\" & \"the potato eaters\""), vec!["\"the potato eaters\""]);
        assert_eq!(vetted_cue_pieces("discovery\" (1804-1806) & sacagawea"), vec!["sacagawea"]);
        assert!(vetted_cue_pieces("NORWAY").is_empty());
        assert!(vetted_cue_pieces("CZECHOSLOVAKIA").is_empty());
        assert_eq!(vetted_cue_pieces("\"bolero\""), vec!["\"bolero\""]);
        assert_eq!(vetted_cue_pieces("1812 Overture"), vec!["1812 Overture"]);
        let prose = "Mark McGwire - Former A's/Cardinals first baseman, prolific power hitter, broke Maris' single-season record. Usually, Home Runs + 90s";
        assert!(vetted_cue_pieces(prose).is_empty());
        assert_eq!(vetted_cue_pieces("goddess of the hearth, & vestal virgins"), vec!["goddess of the hearth", "vestal virgins"]);
    }

    #[test]
    fn frame_grams_are_identity_stems() {
        for g in ["playwright", "compos", "norwegian"] {
            assert!(FRAME_GRAMS.contains(&g), "{g}");
        }
        for g in ["peer gynt", "josephin", "711", "guernica"] {
            assert!(!FRAME_GRAMS.contains(&g), "{g}");
        }
    }

    fn hc(id: i32, cue: &str, support: i64, vetted: bool) -> HookCue {
        HookCue { id, cue: cue.into(), support, vetted }
    }

    #[test]
    fn content_tokens_fold_plurals_and_skip_frames_and_numbers() {
        let t: Vec<String> = cue_content_tokens("14-line poems by Giacomo da Lentini").into_iter().collect();
        assert_eq!(t, vec!["giacomo", "lentini"]);
        let t: Vec<String> = cue_content_tokens("Cubist once said").into_iter().collect();
        assert_eq!(t, vec!["cubist"]);
        let t: Vec<String> = cue_content_tokens("founders of Cubism").into_iter().collect();
        assert_eq!(t, vec!["cubism"]);
        assert!(cue_content_tokens("English playwright of 1598").is_empty());
        let t: Vec<String> = cue_content_tokens("Battle of San Juan Hill").into_iter().collect();
        assert!(t.is_empty(), "battle is a stop word, san/juan/hill too short: {t:?}");
    }

    #[test]
    fn duplicate_pairs_merge_into_the_vetted_or_best_supported_hook() {
        let hooks = vec![
            hc(1, "14-line poems", 17, false),
            hc(2, "the Petrarchan type of these", 6, false),
            hc(3, "14-line poems by Giacomo da Lentini", 3, false),
            hc(4, "Shakespeare wrote 154 of these", 12, false),
        ];
        // "poems" is a stop word, so the two 14-line cues stay apart: nothing shares a content token.
        assert!(duplicate_hook_pairs(&hooks).is_empty());

        let hooks = vec![
            hc(10, "Cubist once said", 36, false),
            hc(11, "his Blue Period", 29, false),
            hc(12, "founder of cubism", 12, true),
            hc(13, "founders of Cubism", 7, false),
            hc(14, "Guernica", 9, false),
        ];
        // cubism ≠ cubist as tokens; the two cubism hooks merge into the vetted one despite lower support.
        assert_eq!(duplicate_hook_pairs(&hooks), vec![(12, 13)]);

        let hooks = vec![
            hc(20, "her husband Pierre discovered radium", 4, false),
            hc(21, "presented with a gram of radium", 11, false),
            hc(22, "daughter Irene", 7, false),
        ];
        assert_eq!(duplicate_hook_pairs(&hooks), vec![(21, 20)]);
        assert!(duplicate_hook_pairs(&[]).is_empty());
    }
}
