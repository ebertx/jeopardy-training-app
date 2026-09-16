//! Entity resolution: which response strings are the same Jeopardy! answer.
//! Spec: docs/superpowers/specs/2026-09-16-jeopardy-objects-design.md §1.
//! Pure — no DB. `objects::run_resolve` feeds it the corpus response forms.

use std::collections::{BTreeMap, HashMap, HashSet};

/// Rust twin of the SQL `lower(trim(regexp_replace(x, '^(the|a|an) ', '', 'i')))`:
/// strips ONE leading article (followed by a space), lowercases, trims.
pub fn norm_response(s: &str) -> String {
    let lower = s.to_lowercase();
    let stripped = ["the ", "an ", "a "]
        .iter()
        .find_map(|p| lower.strip_prefix(p))
        .unwrap_or(&lower);
    stripped.trim().to_string()
}

/// Remove `(...)` and `"..."` segments, collapse runs of whitespace.
pub fn strip_parens(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    let mut in_quote = false;
    for c in s.chars() {
        match c {
            '(' if !in_quote => depth += 1,
            ')' if !in_quote && depth > 0 => depth -= 1,
            '"' if depth == 0 => in_quote = !in_quote,
            _ if depth == 0 && !in_quote => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Strip one leading honorific `Sir ` / `Dame ` (case-insensitive).
pub fn strip_honorific(s: &str) -> String {
    let t = s.trim();
    for h in ["sir ", "dame "] {
        if let Some(prefix) = t.get(..h.len()) {
            if t.len() > h.len() && prefix.eq_ignore_ascii_case(h) {
                return t[h.len()..].trim().to_string();
            }
        }
    }
    t.to_string()
}

/// The entity key of a raw response string.
pub fn entity_key(raw: &str) -> String {
    norm_response(&strip_honorific(&strip_parens(raw)))
}

/// `"(First) Last"` → `Some((first_lower, last_key))`. The parenthetical must
/// open the string; the remainder must be a single-word name (no commas, no
/// digits, no spaces after normalization). Honorifics inside the parens are
/// dropped so `"(Sir Edward) Elgar"` licenses `edward`.
pub fn parenthetical_license(raw: &str) -> Option<(String, String)> {
    let t = raw.trim();
    if !t.starts_with('(') {
        return None;
    }
    let close = t.find(')')?;
    let first = strip_honorific(t[1..close].trim()).to_lowercase();
    let last = norm_response(t[close + 1..].trim());
    if first.is_empty() || last.is_empty() || last.contains(' ') || last.contains(',') {
        return None;
    }
    if first.chars().any(|c| c.is_ascii_digit()) || last.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    if !first.chars().all(|c| c.is_alphabetic() || c == ' ' || c == '-' || c == '.' || c == '\'') {
        return None;
    }
    Some((first, last))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Form {
    pub raw: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    pub key: String,
    pub display: String,
    pub forms: Vec<String>,
    pub freq: i64,
}

/// Group response forms into entities (spec §1 rules):
/// 1. key = entity_key(raw);
/// 2. a multi-token key `first last` collapses to `last` only when some form
///    `"(first) last"` exists in the input (the parenthetical license);
/// 3. display = the form with the most tokens (parentheticals stripped), ties
///    by count; forms = raw strings by count desc; freq = sum of counts.
pub fn resolve(forms: &[Form]) -> Vec<Entity> {
    let mut licenses: HashMap<String, HashSet<String>> = HashMap::new();
    for f in forms {
        if let Some((first, last)) = parenthetical_license(&f.raw) {
            licenses.entry(last).or_default().insert(first);
        }
    }

    let mut groups: BTreeMap<String, Vec<&Form>> = BTreeMap::new();
    for f in forms {
        let base = entity_key(&f.raw);
        if base.is_empty() {
            continue;
        }
        let key = match base.rsplit_once(' ') {
            Some((first, last)) if licenses.get(last).is_some_and(|s| s.contains(first)) => {
                last.to_string()
            }
            _ => base,
        };
        groups.entry(key).or_default().push(f);
    }

    groups
        .into_iter()
        .map(|(key, mut members)| {
            members.sort_by(|a, b| b.count.cmp(&a.count).then(a.raw.cmp(&b.raw)));
            let display = members
                .iter()
                .map(|m| (strip_parens(&m.raw), m.count))
                .filter(|(d, _)| !d.is_empty())
                .max_by(|(da, ca), (db, cb)| {
                    da.split_whitespace().count()
                        .cmp(&db.split_whitespace().count())
                        .then(ca.cmp(cb))
                        .then(db.cmp(da))
                })
                .map(|(d, _)| d)
                .unwrap_or_else(|| key.clone());
            Entity {
                freq: members.iter().map(|m| m.count).sum(),
                forms: members.iter().map(|m| m.raw.clone()).collect(),
                display,
                key,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(raw: &str, count: i64) -> Form { Form { raw: raw.to_string(), count } }
    fn by_key<'a>(ents: &'a [Entity], key: &str) -> &'a Entity {
        ents.iter().find(|e| e.key == key).unwrap_or_else(|| panic!("no entity {key}"))
    }

    #[test]
    fn norm_response_strips_one_article_and_lowercases() {
        assert_eq!(norm_response("The Visigoths"), "visigoths");
        assert_eq!(norm_response("a cage"), "cage");
        assert_eq!(norm_response("  Grieg "), "grieg");
        assert_eq!(norm_response("Theodore Roosevelt"), "theodore roosevelt"); // 'the' only as a word
        assert_eq!(norm_response("  the Visigoths"), "the visigoths"); // SQL anchors ^ on the untrimmed value
    }

    #[test]
    fn strip_parens_removes_parentheticals_and_quoted_nicknames() {
        assert_eq!(strip_parens("(Edvard) Grieg"), "Grieg");
        assert_eq!(strip_parens("Edvard Munch (1863-1944)"), "Edvard Munch");
        assert_eq!(strip_parens("jean \"finlandia\" sibelius"), "jean sibelius");
        assert_eq!(strip_parens("Grieg"), "Grieg");
    }

    #[test]
    fn strip_honorific_removes_sir_and_dame_only() {
        assert_eq!(strip_honorific("Sir Edward Elgar"), "Edward Elgar");
        assert_eq!(strip_honorific("dame Judi Dench"), "Judi Dench");
        assert_eq!(strip_honorific("Saint Peter"), "Saint Peter");
        assert_eq!(strip_honorific("Sirius"), "Sirius");
    }

    #[test]
    fn entity_key_composes_the_three_steps() {
        assert_eq!(entity_key("(Sir Edward) Elgar"), "elgar");
        assert_eq!(entity_key("Sir Edward Elgar"), "edward elgar");
        assert_eq!(entity_key("the Visigoths"), "visigoths");
        assert_eq!(entity_key("(the) Visigoths"), "visigoths");
    }

    #[test]
    fn parenthetical_license_reads_first_and_last() {
        assert_eq!(parenthetical_license("(Edvard) Grieg"), Some(("edvard".into(), "grieg".into())));
        assert_eq!(parenthetical_license("(Ralph Waldo) Emerson"), Some(("ralph waldo".into(), "emerson".into())));
        assert_eq!(parenthetical_license("(Sir Edward) Elgar"), Some(("edward".into(), "elgar".into())));
        assert_eq!(parenthetical_license("Edvard Grieg"), None);
        assert_eq!(parenthetical_license("Edvard Munch (1863-1944)"), None);
        assert_eq!(parenthetical_license("(1 of) Balakirev, Borodin"), None); // last part has a comma: not a name
    }

    #[test]
    fn grieg_forms_merge_into_one_entity() {
        let ents = resolve(&[f("(Edvard) Grieg", 19), f("Edvard Grieg", 19), f("Grieg", 11), f("Edward Grieg", 2)]);
        let g = by_key(&ents, "grieg");
        assert_eq!(g.display, "Edvard Grieg");
        assert_eq!(g.freq, 49);
        assert_eq!(g.forms, vec!["(Edvard) Grieg", "Edvard Grieg", "Grieg"]);
        // misspelled first name is NOT licensed
        assert_eq!(by_key(&ents, "edward grieg").freq, 2);
        assert_eq!(ents.len(), 2);
    }

    #[test]
    fn honorific_forms_merge_under_the_licensed_surname() {
        let ents = resolve(&[f("(Sir Edward) Elgar", 10), f("Sir Edward Elgar", 5), f("Edward Elgar", 8), f("Elgar", 5)]);
        assert_eq!(ents.len(), 1);
        assert_eq!(ents[0].key, "elgar");
        assert_eq!(ents[0].freq, 28);
        assert_eq!(ents[0].display, "Sir Edward Elgar");
    }

    #[test]
    fn multiword_first_names_are_licensed_as_a_unit() {
        let ents = resolve(&[f("(Ralph Waldo) Emerson", 30), f("Ralph Waldo Emerson", 40), f("Emerson", 12)]);
        assert_eq!(ents.len(), 1);
        assert_eq!(ents[0].display, "Ralph Waldo Emerson");
        assert_eq!(ents[0].freq, 82);
    }

    #[test]
    fn unlicensed_compounds_stay_separate() {
        let ents = resolve(&[
            f("Mexico", 387), f("New Mexico", 179),
            f("London", 296), f("Jack London", 70),
            f("Washington", 223), f("Denzel Washington", 32),
        ]);
        assert_eq!(ents.len(), 6);
        assert_eq!(by_key(&ents, "new mexico").freq, 179);
        assert_eq!(by_key(&ents, "jack london").freq, 70);
        assert_eq!(by_key(&ents, "denzel washington").freq, 32);
    }

    #[test]
    fn different_first_name_variants_do_not_merge() {
        let ents = resolve(&[f("(Claude) Debussy", 20), f("Claude Debussy", 20), f("Claude-Achille Debussy", 1)]);
        assert_eq!(ents.len(), 2);
        assert_eq!(by_key(&ents, "debussy").freq, 40);
        assert_eq!(by_key(&ents, "claude-achille debussy").freq, 1);
    }

    #[test]
    fn display_prefers_the_fullest_then_most_frequent_form() {
        let ents = resolve(&[f("(Ludwig van) Beethoven", 60), f("Beethoven", 150), f("Ludwig van Beethoven", 20)]);
        assert_eq!(ents[0].display, "Ludwig van Beethoven");
        assert_eq!(ents[0].forms[0], "Beethoven"); // forms sorted by count desc
    }

    #[test]
    fn output_is_sorted_by_key() {
        let ents = resolve(&[f("Zola", 3), f("Austen", 4)]);
        assert_eq!(ents.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(), vec!["austen", "zola"]);
    }
}
