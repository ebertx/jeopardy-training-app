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

/// A leading parenthetical whose content is name-like (only letters, spaces,
/// `.`, `-`, `'`, `"`, `&`) is part of the name and is kept, parens removed:
/// `"(George) Washington"` → `George Washington`. Every other parenthetical
/// is an annotation and is dropped whole: `"Andrew Jackson (Old Hickory)"` →
/// `Andrew Jackson`; `"(1 of) Spain (or Portugal)"` → `Spain` (the leading
/// content has a digit, so it isn't name-like and is dropped like the
/// rest). Dropping is nested-aware; an unbalanced trailing `(` drops to the
/// end of the string. Quote characters are left alone: the corpus stores
/// quoted titles as ordinary response text (`\"The Raven\"`, `Toys "R" Us`),
/// so a response is not free to lose them here. (Vetted-TSV nicknames get
/// their own quote-stripping helper, `vetted_response_key`, below.)
pub fn strip_parens(s: &str) -> String {
    let t = s.trim();
    if let Some((content, rest)) = leading_paren(t) {
        if is_name_like(content) {
            let combined = format!("{} {}", content.trim(), drop_all_parens(rest));
            return combined.split_whitespace().collect::<Vec<_>>().join(" ");
        }
    }
    drop_all_parens(t)
}

/// If `s` starts with `(`, returns `(content_between_the_parens, rest_after_the_close)`
/// for the matching top-level close (nested-aware). `None` if `s` doesn't
/// open with `(`, or the paren is never closed.
fn leading_paren(s: &str) -> Option<(&str, &str)> {
    if !s.starts_with('(') {
        return None;
    }
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&s[1..i], &s[i + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

/// Only letters, spaces, `.`, `-`, `'`, `"`, `&` — the character set R1
/// treats as "part of a name" rather than an annotation.
fn is_name_like(s: &str) -> bool {
    let t = s.trim();
    !t.is_empty() && t.chars().all(|c| c.is_alphabetic() || matches!(c, ' ' | '.' | '-' | '\'' | '"' | '&'))
}

/// Remove every top-level `(...)` segment (nested-aware; an unbalanced `(`
/// drops to the end of the string), collapse runs of whitespace.
fn drop_all_parens(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
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

/// The entity key of a raw response string. Strips parentheticals and
/// honorifics, then normalizes; but if that strip leaves nothing or only a
/// bare article (a `(...)`-only response, e.g. `(the) Visigoths`), falls
/// back to the plain norm of the raw string. Quote characters are never
/// stripped here — the corpus stores quoted titles as ordinary response
/// text, so `The "Mona Lisa"` and `"Hamlet"` keep their own distinct keys.
pub fn entity_key(raw: &str) -> String {
    let stripped = norm_response(&strip_honorific(&strip_parens(raw)));
    if stripped.is_empty() || matches!(stripped.as_str(), "the" | "a" | "an") {
        return norm_response(raw);
    }
    stripped
}

/// Key for a vetted-list response, which may carry a quoted nickname
/// (`jean "finlandia" sibelius`). Quoted segments are dropped first; if that
/// leaves nothing usable, the quote characters alone are dropped instead.
pub fn vetted_response_key(raw: &str) -> String {
    let unquoted = strip_quoted(raw);
    let key = entity_key(&unquoted);
    if key.is_empty() || matches!(key.as_str(), "the" | "a" | "an") {
        entity_key(&raw.replace('"', ""))
    } else {
        key
    }
}

/// Remove `"..."` segments (unbalanced trailing quote: drop to end), collapse whitespace.
fn strip_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_quote = false;
    for c in s.chars() {
        match c {
            '"' => in_quote = !in_quote,
            _ if !in_quote => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `"(First) Last"` → `Some((first_lower, last_key))`. The parenthetical must
/// open the string; the remainder must be a single-word name (no commas, no
/// digits, no spaces after normalization); the first name must be name-like
/// (R1's rule — letters, spaces, `.`, `-`, `'`, `"`, `&`) after honorifics
/// inside the parens are dropped, so `"(Sir Edward) Elgar"` licenses `edward`.
pub fn parenthetical_license(raw: &str) -> Option<(String, String)> {
    let t = raw.trim();
    if !t.starts_with('(') {
        return None;
    }
    let close = t.find(')')?;
    let first = strip_honorific(t[1..close].trim()).to_lowercase();
    let last = norm_response(t[close + 1..].trim());
    if last.is_empty() || last.contains(' ') || last.contains(',') || last.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    if !is_name_like(&first) {
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

/// Group response forms into entities (controller ruling, spec §1 to be
/// amended to match):
/// 1. key = entity_key(raw) — always the full name (R2): a leading name-like
///    parenthetical is already folded into it by `strip_parens`, so
///    `"(Edvard) Grieg"` and `"Edvard Grieg"` share the key `edvard grieg`.
/// 2. a single-token (bare-surname) key `s` is absorbed into a multi-token
///    key `f s` only when BOTH hold (R3): exactly one distinct first name
///    `f` is licensed for `s` (via `parenthetical_license` on some form
///    `"(f) s"`), and the bare form isn't the dominant usage — its count
///    does not exceed the summed count of the `f s` forms. Otherwise the
///    bare form stays its own entity, so an ambiguous or dominant surname
///    (`Washington`, `London`) never swallows unrelated answers.
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
        let key = entity_key(&f.raw);
        if key.is_empty() {
            continue;
        }
        groups.entry(key).or_default().push(f);
    }

    // Bare-surname absorption (R3): a single-token key with exactly one
    // licensed first name is folded into that full-name key, unless the
    // bare form's own count already dominates the full-name forms' total.
    let freq_by_key: HashMap<&str, i64> =
        groups.iter().map(|(k, members)| (k.as_str(), members.iter().map(|m| m.count).sum())).collect();
    let mut absorbed_into: HashMap<String, String> = HashMap::new();
    for key in groups.keys() {
        if key.contains(' ') {
            continue; // already a full name, nothing to absorb it into
        }
        let Some(firsts) = licenses.get(key) else { continue };
        if firsts.len() != 1 {
            continue; // ambiguous: more than one licensed first name
        }
        let first = firsts.iter().next().unwrap();
        let full_key = format!("{first} {key}");
        let Some(&full_freq) = freq_by_key.get(full_key.as_str()) else { continue };
        if freq_by_key[key.as_str()] <= full_freq {
            absorbed_into.insert(key.clone(), full_key);
        }
    }

    let mut merged: BTreeMap<String, Vec<&Form>> = BTreeMap::new();
    for (key, members) in groups {
        let target = absorbed_into.remove(&key).unwrap_or(key);
        merged.entry(target).or_default().extend(members);
    }

    merged
        .into_iter()
        .map(|(key, mut members)| {
            members.sort_by(|a, b| b.count.cmp(&a.count).then(a.raw.cmp(&b.raw)));
            // R5: display is the most frequent surviving stripped form among
            // this entity's OWN forms — "surviving" meaning stripping and
            // re-keying it lands back on this entity's key, which excludes a
            // bare surname absorbed from elsewhere (its own key differs).
            // Identical stripped strings pool their counts; ties keep the
            // alphabetically-first string, via BTreeMap's ascending order.
            let mut counts: BTreeMap<String, i64> = BTreeMap::new();
            for m in &members {
                let stripped = strip_parens(&m.raw);
                if stripped.is_empty() || entity_key(&stripped) != key {
                    continue;
                }
                *counts.entry(stripped).or_insert(0) += m.count;
            }
            let display = counts
                .into_iter()
                .fold(None::<(String, i64)>, |best, (s, c)| match best {
                    Some((bs, bc)) if bc >= c => Some((bs, bc)),
                    _ => Some((s, c)),
                })
                .map(|(s, _)| s)
                .unwrap_or_else(|| {
                    // Should be unreachable for a well-formed group: fall
                    // back to the old rule (most tokens, then count).
                    members
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
                        .unwrap_or_else(|| key.clone())
                });
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
    fn leading_name_parenthetical_is_kept_trailing_ones_dropped() {
        assert_eq!(strip_parens("(George) Washington"), "George Washington");
        assert_eq!(strip_parens("(Sir Edward) Elgar"), "Sir Edward Elgar");
        assert_eq!(strip_parens("(University of) Chicago"), "University of Chicago");
        assert_eq!(strip_parens("Andrew Jackson (Old Hickory)"), "Andrew Jackson");
        assert_eq!(strip_parens("Mexico (Mexico City)"), "Mexico");
        assert_eq!(strip_parens("George (H.W.) Bush"), "George Bush");
        assert_eq!(strip_parens("(1 of) Spain (or Portugal)"), "Spain");
        assert_eq!(strip_parens("Edvard Munch (1863-1944)"), "Edvard Munch");
        assert_eq!(strip_parens("Grieg"), "Grieg");
        assert_eq!(strip_parens("jean \"finlandia\" sibelius"), "jean \"finlandia\" sibelius");
    }

    #[test]
    fn strip_honorific_removes_sir_and_dame_only() {
        assert_eq!(strip_honorific("Sir Edward Elgar"), "Edward Elgar");
        assert_eq!(strip_honorific("dame Judi Dench"), "Judi Dench");
        assert_eq!(strip_honorific("Saint Peter"), "Saint Peter");
        assert_eq!(strip_honorific("Sirius"), "Sirius");
    }

    #[test]
    fn entity_key_is_the_full_name() {
        assert_eq!(entity_key("(Edvard) Grieg"), "edvard grieg");
        assert_eq!(entity_key("(Sir Edward) Elgar"), "edward elgar");
        assert_eq!(entity_key("Sir Edward Elgar"), "edward elgar");
        assert_eq!(entity_key("the Visigoths"), "visigoths");
        assert_eq!(entity_key("(the) Visigoths"), "visigoths");
        assert_eq!(entity_key("Andrew Jackson (Old Hickory)"), "andrew jackson");
    }

    #[test]
    fn quoted_titles_keep_their_own_key() {
        assert_eq!(entity_key("The \"Mona Lisa\""), "\"mona lisa\"");
        assert_eq!(entity_key("\"Hamlet\""), "\"hamlet\"");
        assert_eq!(entity_key("a \"Streetcar Named Desire\""), "\"streetcar named desire\"");
        assert_eq!(entity_key("5\" floppy disk"), "5\" floppy disk");
    }

    #[test]
    fn quoted_titles_never_merge_together() {
        let ents = resolve(&[f("The \"Mona Lisa\"", 40), f("the \"Scream\"", 30), f("\"Hamlet\"", 20)]);
        assert_eq!(ents.len(), 3);
    }

    #[test]
    fn escaped_corpus_quotes_stay_distinct_entities() {
        // The corpus stores titles as \"The Raven\" (backslash-escaped quotes).
        assert_eq!(entity_key("\\\"The Raven\\\""), "\\\"the raven\\\"");
        assert_eq!(entity_key("Toys \"R\" Us"), "toys \"r\" us");
        let ents = resolve(&[f("\\\"The Raven\\\"", 22), f("\\\"American Pie\\\"", 21), f("\\\"Weird Al\" Yankovic", 17), f("Toys \"R\" Us", 18)]);
        assert_eq!(ents.len(), 4);
    }

    #[test]
    fn vetted_response_key_drops_nicknames_but_keeps_quoted_titles() {
        assert_eq!(vetted_response_key("jean \"finlandia\" sibelius"), "jean sibelius");
        assert_eq!(vetted_response_key("edvard \"peer gynt\" grieg"), "edvard grieg");
        assert_eq!(vetted_response_key("edvard munch (1863-1944)"), "edvard munch");
        assert_eq!(vetted_response_key("\"lullaby\""), "lullaby");
        assert_eq!(vetted_response_key("the \"raven\""), "raven");
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
        let g = by_key(&ents, "edvard grieg");
        assert_eq!(g.display, "Edvard Grieg");
        assert_eq!(g.freq, 49);
        assert_eq!(g.forms, vec!["(Edvard) Grieg", "Edvard Grieg", "Grieg"]);
        assert_eq!(by_key(&ents, "edward grieg").freq, 2);
        assert_eq!(ents.len(), 2);
    }

    #[test]
    fn honorific_forms_merge_under_the_full_name() {
        let ents = resolve(&[f("(Sir Edward) Elgar", 10), f("Sir Edward Elgar", 5), f("Edward Elgar", 8), f("Elgar", 5)]);
        assert_eq!(ents.len(), 1);
        assert_eq!(ents[0].key, "edward elgar");
        assert_eq!(ents[0].freq, 28);
        assert_eq!(ents[0].display, "Sir Edward Elgar");
    }

    #[test]
    fn multiword_first_names_are_licensed_as_a_unit() {
        let ents = resolve(&[f("(Ralph Waldo) Emerson", 30), f("Ralph Waldo Emerson", 40), f("Emerson", 12)]);
        assert_eq!(ents.len(), 1);
        assert_eq!(ents[0].key, "ralph waldo emerson");
        assert_eq!(ents[0].freq, 82);
    }

    #[test]
    fn ambiguous_surname_never_absorbs_the_bare_form() {
        let ents = resolve(&[f("(George) Washington", 40), f("George Washington", 200), f("(Denzel) Washington", 5), f("Denzel Washington", 30), f("Washington", 250)]);
        assert_eq!(ents.len(), 3);
        assert_eq!(by_key(&ents, "washington").freq, 250);
        assert_eq!(by_key(&ents, "george washington").freq, 240);
        assert_eq!(by_key(&ents, "denzel washington").freq, 35);
    }

    #[test]
    fn dominant_bare_form_stays_separate() {
        // "(Jack) London" licenses jack, but London the city dominates.
        let ents = resolve(&[f("London", 296), f("Jack London", 70), f("(Jack) London", 17)]);
        assert_eq!(ents.len(), 2);
        assert_eq!(by_key(&ents, "london").freq, 296);
        assert_eq!(by_key(&ents, "jack london").freq, 87);
        // Beethoven: bare 150 > 60 + 20, so it stays separate too (a missed merge, never a wrong one).
        let ents = resolve(&[f("(Ludwig van) Beethoven", 60), f("Beethoven", 150), f("Ludwig van Beethoven", 20)]);
        assert_eq!(ents.len(), 2);
        assert_eq!(by_key(&ents, "ludwig van beethoven").display, "Ludwig van Beethoven");
        assert_eq!(by_key(&ents, "ludwig van beethoven").forms[0], "(Ludwig van) Beethoven");
    }

    #[test]
    fn unlicensed_compounds_stay_separate() {
        let ents = resolve(&[f("Mexico", 387), f("New Mexico", 179), f("Paris", 300), f("the Treaty of Paris", 40), f("(Treaty of) Paris", 10)]);
        assert_eq!(ents.len(), 4);
        assert_eq!(by_key(&ents, "new mexico").freq, 179);
        assert_eq!(by_key(&ents, "treaty of paris").freq, 50);
        assert_eq!(by_key(&ents, "paris").freq, 300);
    }

    #[test]
    fn different_first_name_variants_do_not_merge() {
        let ents = resolve(&[f("(Claude) Debussy", 20), f("Claude Debussy", 20), f("Claude-Achille Debussy", 1), f("Debussy", 3)]);
        assert_eq!(ents.len(), 2);
        assert_eq!(by_key(&ents, "claude debussy").freq, 43);
        assert_eq!(by_key(&ents, "claude-achille debussy").freq, 1);
    }

    #[test]
    fn display_is_the_most_frequent_form_of_the_full_name() {
        let ents = resolve(&[f("France", 460), f("the France", 1), f("France (or England)", 2)]);
        assert_eq!(ents.len(), 1);
        assert_eq!(ents[0].display, "France");
        let ents = resolve(&[f("Napoleon", 250), f("a Napoleon", 1), f("Napoleon (Bonaparte)", 6)]);
        assert_eq!(ents[0].display, "Napoleon");
        let ents = resolve(&[f("the Netherlands", 200), f("Netherlands", 40), f("The Netherlands (Holland)", 3)]);
        assert_eq!(ents[0].display, "the Netherlands");
    }

    #[test]
    fn output_is_sorted_by_key() {
        let ents = resolve(&[f("Zola", 3), f("Austen", 4)]);
        assert_eq!(ents.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(), vec!["austen", "zola"]);
    }
}
