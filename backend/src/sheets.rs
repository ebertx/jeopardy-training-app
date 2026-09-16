//! Answer sheets: a corpus-grounded identity + four drillable facts per answer.
//! Pure logic here (prompt, parsing, normalization); generation in the second half.

use serde::Serialize;
use serde_json::Value;

pub const SHEET_MODEL: &str = "gpt-4o-mini";

pub const SHEET_SYSTEM_PROMPT: &str = r#"You are a Jeopardy! coach building an ANSWER SHEET: the handful of facts that let a player recognize this answer from any clue angle.

Rules:
- Output ONLY valid JSON: {"identity": "...", "facts": [{"prompt": "...", "response": "..."}, ...]} with EXACTLY four facts.
- "identity": one sentence, at most 25 words, saying who or what this answer is. Proper nouns, dates and numbers only. Never restate a clue.
- Each fact is something Jeopardy writers re-use about this answer, drawn from the supplied clues where possible: creator or author, signature work, key date or first, place, counterpart or rival, nickname, unit or symbol.
- "prompt" is a 2-6 word label as it would appear in a cue: "author", "narrator", "year published", "capital city", "wrote the music".
- "response" is a short gradeable answer — a name, number, place or title — at most 6 words. Never a sentence. Never the answer itself.
- The four facts must be distinct from each other and must NOT repeat the existing cue phrases you are given; they add depth beyond the cue.
- BANNED: generic sentences about trivia, Jeopardy or "recognizing"; hedges such as "possibly" or "often"."#;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Fact {
    pub prompt: String,
    pub response: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SheetContent {
    pub identity: String,
    pub facts: Vec<Fact>,
}

/// A clue used to ground the sheet. `clue` is the clue text shown to players
/// (jeopardy_questions.answer), `year` from air_date.
#[derive(Debug, Clone)]
pub struct GroundingClue {
    pub clue: String,
    pub category: String,
    pub year: Option<i32>,
}

/// Rust twin of the SQL `lower(trim(regexp_replace(question, '^(the|a|an) ', '', 'i')))`.
/// Strips ONE leading article (followed by a space), lowercases, trims.
pub fn normalize_answer(s: &str) -> String {
    let lower = s.to_lowercase();
    let stripped = ["the ", "an ", "a "]
        .iter()
        .find_map(|p| lower.strip_prefix(p))
        .unwrap_or(&lower);
    stripped.trim().to_string()
}

/// answer_norm of the n-th fact card under a parent (n = 1..=4).
pub fn fact_norm(parent_norm: &str, n: usize) -> String {
    format!("{parent_norm}::{n}")
}

pub fn sheet_user_prompt(
    answer: &str,
    category: &str,
    phrases: &[String],
    clues: &[GroundingClue],
) -> String {
    let mut out = format!("Answer: \"{answer}\"\nCategory: {category}\n");
    out.push_str(&format!(
        "Existing cue phrases: {}\n",
        if phrases.is_empty() { "(none)".to_string() } else { phrases.join(", ") }
    ));
    out.push_str("Clues Jeopardy has written for this answer:\n");
    for c in clues {
        match c.year {
            Some(y) => out.push_str(&format!("- \"{}\" ({}, {})\n", c.clue, c.category, y)),
            None => out.push_str(&format!("- \"{}\" ({})\n", c.clue, c.category)),
        }
    }
    out.push_str("\nReturn the JSON now.");
    out
}

/// Validate the LLM's JSON. Rejections are never cached.
pub fn parse_sheet(v: &Value, answer_norm: &str) -> Result<SheetContent, String> {
    let identity = v
        .get("identity")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("missing or empty identity")?
        .to_string();
    let arr = v.get("facts").and_then(|f| f.as_array()).ok_or("missing facts")?;
    if arr.len() != 4 {
        return Err(format!("expected 4 facts, got {}", arr.len()));
    }
    let mut facts = Vec::with_capacity(4);
    let mut seen_prompts = std::collections::HashSet::new();
    for f in arr {
        let get = |k: &str| -> Result<String, String> {
            let s = f
                .get(k)
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| format!("fact missing {k}"))?;
            Ok(s.to_string())
        };
        let prompt = get("prompt")?;
        let response = get("response")?;
        if normalize_answer(&response) == answer_norm {
            return Err(format!("fact response restates the answer: {response}"));
        }
        if !seen_prompts.insert(prompt.to_lowercase()) {
            return Err(format!("duplicate fact prompt: {prompt}"));
        }
        facts.push(Fact { prompt, response });
    }
    Ok(SheetContent { identity, facts })
}

/// Pick `n` items spread evenly across a chronologically ordered slice, always
/// keeping the first and last. Returns everything when the slice has ≤ n items.
pub fn sample_evenly<T: Clone>(items: &[T], n: usize) -> Vec<T> {
    if items.len() <= n || n == 0 {
        return items.to_vec();
    }
    let last = items.len() - 1;
    (0..n)
        .map(|i| items[(i * last) / (n - 1)].clone())
        .collect()
}

pub fn sheet_json(s: &SheetContent) -> Value {
    serde_json::json!({ "identity": s.identity, "facts": s.facts })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn good() -> Value {
        json!({
            "identity": "Emily Brontë's only novel, 1847, a doomed romance on the Yorkshire moors.",
            "facts": [
                {"prompt": "author", "response": "Emily Brontë"},
                {"prompt": "antihero", "response": "Heathcliff"},
                {"prompt": "setting", "response": "Yorkshire moors"},
                {"prompt": "year published", "response": "1847"}
            ]
        })
    }

    #[test]
    fn normalize_matches_the_sql_expression() {
        assert_eq!(normalize_answer("The Jungle"), "jungle");
        assert_eq!(normalize_answer("An Apple"), "apple");
        assert_eq!(normalize_answer("A Midsummer Night's Dream"), "midsummer night's dream");
        assert_eq!(normalize_answer("Aesop"), "aesop"); // no article stripped
        assert_eq!(normalize_answer("  Wuthering Heights "), "wuthering heights");
    }

    #[test]
    fn fact_norm_shape() {
        assert_eq!(fact_norm("wuthering heights", 3), "wuthering heights::3");
    }

    #[test]
    fn parse_accepts_a_valid_sheet() {
        let s = parse_sheet(&good(), "wuthering heights").unwrap();
        assert_eq!(s.facts.len(), 4);
        assert_eq!(s.facts[0].prompt, "author");
        assert_eq!(s.facts[3].response, "1847");
        assert!(s.identity.starts_with("Emily"));
    }

    #[test]
    fn parse_rejects_wrong_fact_count() {
        let mut v = good();
        v["facts"].as_array_mut().unwrap().pop();
        assert!(parse_sheet(&v, "wuthering heights").is_err());
        let mut v = good();
        v["facts"].as_array_mut().unwrap().push(json!({"prompt": "x", "response": "y"}));
        assert!(parse_sheet(&v, "wuthering heights").is_err());
    }

    #[test]
    fn parse_rejects_empty_fields() {
        let mut v = good();
        v["facts"][1]["response"] = json!("  ");
        assert!(parse_sheet(&v, "wuthering heights").is_err());
        let mut v = good();
        v["identity"] = json!("");
        assert!(parse_sheet(&v, "wuthering heights").is_err());
    }

    #[test]
    fn parse_rejects_response_equal_to_answer() {
        let mut v = good();
        v["facts"][2]["response"] = json!("The Wuthering Heights");
        assert!(parse_sheet(&v, "wuthering heights").is_err());
    }

    #[test]
    fn parse_rejects_duplicate_prompts() {
        let mut v = good();
        v["facts"][2]["prompt"] = json!("Author");
        assert!(parse_sheet(&v, "wuthering heights").is_err());
    }

    #[test]
    fn user_prompt_lists_answer_phrases_and_clues() {
        let clues = vec![
            GroundingClue { clue: "Heathcliff loves Cathy here".into(), category: "NOVELS".into(), year: Some(2001) },
            GroundingClue { clue: "Emily's only novel".into(), category: "BRONTËS".into(), year: None },
        ];
        let p = sheet_user_prompt("Wuthering Heights", "Literature & Language", &["Heathcliff".to_string()], &clues);
        assert!(p.contains("Answer: \"Wuthering Heights\""));
        assert!(p.contains("Category: Literature & Language"));
        assert!(p.contains("Existing cue phrases: Heathcliff"));
        assert!(p.contains("- \"Heathcliff loves Cathy here\" (NOVELS, 2001)"));
        assert!(p.contains("- \"Emily's only novel\" (BRONTËS)"));
        assert!(p.trim_end().ends_with("Return the JSON now."));
    }

    #[test]
    fn sample_evenly_keeps_ends_and_count() {
        let v: Vec<i32> = (0..30).collect();
        let s = sample_evenly(&v, 15);
        assert_eq!(s.len(), 15);
        assert_eq!(s[0], 0);
        assert_eq!(*s.last().unwrap(), 29);
        assert_eq!(sample_evenly(&v, 100).len(), 30); // fewer than n → all
        assert!(sample_evenly(&Vec::<i32>::new(), 5).is_empty());
    }

    #[test]
    fn sheet_json_round_trips() {
        let s = parse_sheet(&good(), "wuthering heights").unwrap();
        let v = sheet_json(&s);
        assert_eq!(v["facts"].as_array().unwrap().len(), 4);
        assert_eq!(v["facts"][0]["prompt"], "author");
        assert!(parse_sheet(&v, "wuthering heights").is_ok());
    }
}
