//! Pavlov cue mining: seat planning, TF-IDF term filtering, and LLM polish
//! (docs/superpowers/specs/2026-07-21-pavlov-cues-design.md).

use crate::blend::{sampling_kind, split_seats, SamplingKind, TARGET_WEIGHTS};
use crate::routes::mock_test::apportion;

pub const TOTAL_SEATS: i64 = 1500;
pub const MIN_FREQ: i32 = 5;

#[derive(Debug, Clone)]
pub struct SeatPlan {
    pub category: String,
    pub canon: i64,
    pub recency: i64,
}

pub fn seat_plan(total: i64) -> Vec<SeatPlan> {
    let dist: Vec<(String, i64)> = TARGET_WEIGHTS
        .iter()
        .map(|(c, w)| (c.to_string(), *w))
        .collect();
    apportion(&dist, total)
        .into_iter()
        .map(|(category, seats)| match sampling_kind(&category) {
            SamplingKind::Canon => SeatPlan { category, canon: seats, recency: 0 },
            SamplingKind::Recency => SeatPlan { category, canon: 0, recency: seats },
            SamplingKind::Split => {
                let (canon, recency) = split_seats(seats);
                SeatPlan { category, canon, recency }
            }
        })
        .collect()
}

pub const POLISH_MODEL: &str = "gpt-4o";

/// Drop mined terms that are just stems/variants of the answer itself (the SQL
/// stage already removed exact lexeme matches; this catches near-variants).
/// Rule: a term is self-referential when it shares a common prefix of ≥ 4
/// chars with an answer word AND one is a prefix of the other (case-insensitive).
pub fn filter_self_terms(answer: &str, terms: Vec<String>) -> Vec<String> {
    let answer_words: Vec<String> = answer
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 4)
        .map(|w| w.to_string())
        .collect();
    terms
        .into_iter()
        .filter(|t| {
            let tl = t.to_lowercase();
            !answer_words.iter().any(|w| {
                (tl.starts_with(w.as_str()) || w.starts_with(tl.as_str()))
                    && tl.len().min(w.len()) >= 4
            })
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct PolishInput {
    pub answer: String,
    pub terms: Vec<String>,
    pub sample_clues: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PolishOutcome {
    pub answer: String,
    pub keep: bool,
    pub phrases: Vec<String>,
}

/// (system, user) prompts for one polish batch. The system prompt pins the
/// JSON shape; the user prompt carries the mined evidence per answer.
pub fn polish_prompts(batch: &[PolishInput]) -> (String, String) {
    let system = "You turn mined Jeopardy! writer-habit data into study flashcards. \
For each answer you receive its most distinctive clue keywords (stemmed) and sample real clues. \
Write 2-4 short human-readable cue phrases per answer — the trigger associations a contestant \
should learn (e.g. for Solomon: \"wise king\", \"Ecclesiastes ascribed to\"). \
Every phrase must be grounded in the given keywords or sample clues; never invent associations. \
Set keep=false when the keywords are too generic or self-referential to make useful cues. \
Respond with JSON only: {\"results\": [{\"answer\": string (echoed verbatim), \
\"keep\": boolean, \"cue_phrases\": [string]}]}"
        .to_string();

    let items: Vec<serde_json::Value> = batch
        .iter()
        .map(|b| {
            serde_json::json!({
                "answer": b.answer,
                "mined_keywords": b.terms,
                "sample_clues": b.sample_clues,
            })
        })
        .collect();
    let user = serde_json::to_string_pretty(&serde_json::json!({ "answers": items }))
        .expect("serializable");
    (system, user)
}

/// Lenient parse: items without an answer string are skipped; phrases are
/// trimmed, de-blanked, capped at 4; keep with < 2 phrases demotes to dropped.
pub fn parse_polish_response(v: &serde_json::Value) -> Vec<PolishOutcome> {
    let Some(results) = v.get("results").and_then(|r| r.as_array()) else {
        return vec![];
    };
    results
        .iter()
        .filter_map(|item| {
            let answer = item.get("answer")?.as_str()?.trim().to_string();
            if answer.is_empty() {
                return None;
            }
            let mut phrases: Vec<String> = item
                .get("cue_phrases")
                .and_then(|p| p.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| s.as_str())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            phrases.truncate(4);
            let keep = item.get("keep").and_then(|k| k.as_bool()).unwrap_or(true)
                && phrases.len() >= 2;
            Some(PolishOutcome { answer, keep, phrases })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_for(cat: &str, plan: &[SeatPlan]) -> (i64, i64) {
        let p = plan.iter().find(|p| p.category == cat).expect("category present");
        (p.canon, p.recency)
    }

    #[test]
    fn seat_plan_covers_all_categories_and_sums_to_total() {
        let plan = seat_plan(1500);
        assert_eq!(plan.len(), TARGET_WEIGHTS.len());
        let sum: i64 = plan.iter().map(|p| p.canon + p.recency).sum();
        assert_eq!(sum, 1500);
    }

    #[test]
    fn canon_categories_get_only_canon_seats() {
        let plan = seat_plan(1500);
        // Literature & Language is 20/100 of 1500 = 300, all canon.
        assert_eq!(plan_for("Literature & Language", &plan), (300, 0));
    }

    #[test]
    fn recency_categories_get_only_recency_seats() {
        let plan = seat_plan(1500);
        // Film, TV & Pop Culture is 10/100 of 1500 = 150, all recency.
        assert_eq!(plan_for("Film, TV & Pop Culture", &plan), (0, 150));
    }

    #[test]
    fn music_splits_seats_with_canon_taking_the_odd_one() {
        let plan = seat_plan(1500);
        // Music & Performing Arts is 6/100 of 1500 = 90 → 45/45.
        let (canon, recency) = plan_for("Music & Performing Arts", &plan);
        assert_eq!(canon + recency, 90);
        assert!(canon >= recency);
        assert!(canon - recency <= 1);
    }

    #[test]
    fn filter_self_terms_drops_stems_of_the_answer() {
        let terms = vec![
            "hemingway".to_string(), // shares ≥4-char prefix with answer word
            "bell".to_string(),      // < 4 chars overlap requirement, kept
            "spanish".to_string(),
        ];
        let kept = filter_self_terms("Ernest Hemingway", terms);
        assert_eq!(kept, vec!["bell".to_string(), "spanish".to_string()]);
    }

    #[test]
    fn filter_self_terms_is_case_insensitive_and_keeps_order() {
        let kept = filter_self_terms(
            "Solomon",
            vec!["wise".into(), "SOLOMONS".into(), "king".into()],
        );
        assert_eq!(kept, vec!["wise".to_string(), "king".to_string()]);
    }

    #[test]
    fn polish_prompts_mention_every_answer_and_demand_json() {
        let batch = vec![PolishInput {
            answer: "Solomon".into(),
            terms: vec!["wise".into(), "king".into(), "ecclesiast".into()],
            sample_clues: vec!["The book of Ecclesiastes is traditionally ascribed to this wise king".into()],
        }];
        let (system, user) = polish_prompts(&batch);
        assert!(system.contains("JSON"));
        assert!(user.contains("Solomon"));
        assert!(user.contains("ecclesiast"));
        assert!(user.contains("wise king")); // sample clue included
    }

    #[test]
    fn parse_polish_response_accepts_wellformed_and_enforces_phrase_floor() {
        let v = serde_json::json!({
            "results": [
                { "answer": "Solomon", "keep": true,
                  "cue_phrases": ["wise king", "Ecclesiastes ascribed to", "Temple builder"] },
                { "answer": "Junk", "keep": true, "cue_phrases": ["only one"] },
                { "answer": "Generic", "keep": false, "cue_phrases": [] }
            ]
        });
        let out = parse_polish_response(&v);
        assert_eq!(out.len(), 3);
        assert!(out[0].keep);
        assert_eq!(out[0].phrases.len(), 3);
        assert!(!out[1].keep, "keep with <2 phrases is demoted to dropped");
        assert!(!out[2].keep);
    }

    #[test]
    fn parse_polish_response_caps_phrases_at_four_and_skips_nameless_items() {
        let v = serde_json::json!({
            "results": [
                { "keep": true, "cue_phrases": ["a", "b"] },
                { "answer": "Nile", "keep": true,
                  "cue_phrases": ["longest river", "Egypt", "Aswan", "Khartoum", "delta"] }
            ]
        });
        let out = parse_polish_response(&v);
        assert_eq!(out.len(), 1, "item without an answer string is skipped");
        assert_eq!(out[0].phrases.len(), 4);
    }

    #[test]
    fn parse_polish_response_of_garbage_is_empty() {
        assert!(parse_polish_response(&serde_json::json!({"nope": 1})).is_empty());
        assert!(parse_polish_response(&serde_json::json!("string")).is_empty());
    }
}
