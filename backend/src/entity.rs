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

/// Count-weighted mode of a set of (category, count) pairs; ties keep the
/// alphabetically-first category (via `BTreeMap`'s ascending iteration
/// order). An empty string is an ordinary candidate here — it's R3′'s
/// absorption guard, not this function, that treats "no dominant category"
/// as never matching anything.
fn dominant_category<'a>(items: impl Iterator<Item = (&'a str, i64)>) -> String {
    let mut counts: BTreeMap<&str, i64> = BTreeMap::new();
    for (cat, c) in items {
        *counts.entry(cat).or_insert(0) += c;
    }
    counts
        .into_iter()
        .fold(None::<(&str, i64)>, |best, (cat, c)| match best {
            Some((bc_cat, bc)) if bc >= c => Some((bc_cat, bc)),
            _ => Some((cat, c)),
        })
        .map(|(cat, _)| cat.to_string())
        .unwrap_or_default()
}

/// R3′(c): a bare single-token key's form is eligible to move into a
/// licensed full-name entity only if it plainly names the person by itself —
/// a single token, starting with an uppercase letter, with no parenthesis or
/// quote (and, being a single token, trivially no leading `the `/`a `/`an `
/// either). Lowercase and article-led forms almost always name a different,
/// unrelated sense of the same word (`"temple"` the building vs. `"Temple"`
/// the surname) and are never moved.
fn is_eligible_bare_form(raw: &str) -> bool {
    let t = raw.trim();
    t.split_whitespace().count() == 1
        && !t.chars().any(|c| matches!(c, '(' | ')' | '"'))
        && t.chars().next().is_some_and(|c| c.is_uppercase())
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
    /// The dominant `classifier_category` of this form's clues; "" when none.
    pub category: String,
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
/// 2. a single-token (bare-surname) key `s` splits into eligible forms
///    (single-token, uppercase-led, no parens/quotes — plainly the name by
///    itself, e.g. `"Caesar"`) and ineligible ones (`"caesar"`, `"a Caesar"`,
///    `"the temple"` — almost always a different, unrelated sense of the
///    same word). Only the eligible subset is ever a candidate to move into
///    a multi-token key `f s`, and only when BOTH hold (R3′): exactly one
///    distinct first name `f` is licensed for `s` (via `parenthetical_license`
///    on some form `"(f) s"`), and the eligible subset's dominant category
///    (count-weighted mode) equals `f s`'s dominant category and is
///    non-empty. The ineligible subset always stays behind under key `s`;
///    if the guards fail, the eligible subset stays there too. This is why
///    an ambiguous surname (`Washington`) or a category mismatch (`London`
///    the place vs. `Jack London`) never swallows unrelated answers, while a
///    same-category surname (`Beethoven`) is absorbed even when the bare
///    form is more frequent than the full name.
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

    // Every key's own dominant category (count-weighted mode), needed as the
    // right-hand side of the R3′ absorption guard. Owned (not borrowed from
    // `groups`) so it survives the `groups` consuming loop below.
    let category_by_key: HashMap<String, String> = groups
        .iter()
        .map(|(k, members)| {
            (k.clone(), dominant_category(members.iter().map(|m| (m.category.as_str(), m.count))))
        })
        .collect();

    // Bare-surname absorption (R3′): see the doc comment above.
    let mut merged: BTreeMap<String, Vec<&Form>> = BTreeMap::new();
    for (key, members) in groups {
        if key.contains(' ') {
            merged.entry(key).or_default().extend(members);
            continue; // already a full name, nothing to absorb it into
        }
        let (eligible, ineligible): (Vec<&Form>, Vec<&Form>) =
            members.into_iter().partition(|m| is_eligible_bare_form(&m.raw));

        let absorption_target = licenses
            .get(&key)
            .filter(|firsts| firsts.len() == 1) // exactly one licensed first name
            .and_then(|firsts| {
                if eligible.is_empty() {
                    return None;
                }
                let first = firsts.iter().next().unwrap();
                let full_key = format!("{first} {key}");
                let full_cat = category_by_key.get(&full_key)?;
                let eligible_cat =
                    dominant_category(eligible.iter().map(|m| (m.category.as_str(), m.count)));
                if eligible_cat.is_empty() || eligible_cat != *full_cat {
                    return None; // no category match, or no category at all
                }
                Some(full_key)
            });

        match absorption_target {
            Some(full_key) => {
                merged.entry(full_key).or_default().extend(eligible);
                if !ineligible.is_empty() {
                    merged.entry(key).or_default().extend(ineligible);
                }
            }
            None => {
                let mut all = eligible;
                all.extend(ineligible);
                merged.entry(key).or_default().extend(all);
            }
        }
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

    fn f(raw: &str, count: i64) -> Form { Form { raw: raw.to_string(), count, category: String::new() } }
    fn fc(raw: &str, count: i64, cat: &str) -> Form {
        Form { raw: raw.to_string(), count, category: cat.to_string() }
    }
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
        let ents = resolve(&[
            fc("(Edvard) Grieg", 19, "Music & Performing Arts"),
            fc("Edvard Grieg", 19, "Music & Performing Arts"),
            fc("Grieg", 11, "Music & Performing Arts"),
            fc("Edward Grieg", 2, "Music & Performing Arts"),
        ]);
        let g = by_key(&ents, "edvard grieg");
        assert_eq!(g.display, "Edvard Grieg");
        assert_eq!(g.freq, 49);
        assert_eq!(g.forms, vec!["(Edvard) Grieg", "Edvard Grieg", "Grieg"]);
        assert_eq!(by_key(&ents, "edward grieg").freq, 2);
        assert_eq!(ents.len(), 2);
    }

    #[test]
    fn honorific_forms_merge_under_the_full_name() {
        let ents = resolve(&[
            fc("(Sir Edward) Elgar", 10, "Music & Performing Arts"),
            fc("Sir Edward Elgar", 5, "Music & Performing Arts"),
            fc("Edward Elgar", 8, "Music & Performing Arts"),
            fc("Elgar", 5, "Music & Performing Arts"),
        ]);
        assert_eq!(ents.len(), 1);
        assert_eq!(ents[0].key, "edward elgar");
        assert_eq!(ents[0].freq, 28);
        assert_eq!(ents[0].display, "Sir Edward Elgar");
    }

    #[test]
    fn multiword_first_names_are_licensed_as_a_unit() {
        let ents = resolve(&[
            fc("(Ralph Waldo) Emerson", 30, "Literature & Language"),
            fc("Ralph Waldo Emerson", 40, "Literature & Language"),
            fc("Emerson", 12, "Literature & Language"),
        ]);
        assert_eq!(ents.len(), 1);
        assert_eq!(ents[0].key, "ralph waldo emerson");
        assert_eq!(ents[0].freq, 82);
    }

    #[test]
    fn ambiguous_surname_never_absorbs_the_bare_form() {
        // Same category throughout, so only guard (a) — the ambiguous
        // license — is what keeps these apart.
        let ents = resolve(&[
            fc("(George) Washington", 40, "American History"),
            fc("George Washington", 200, "American History"),
            fc("(Denzel) Washington", 5, "American History"),
            fc("Denzel Washington", 30, "American History"),
            fc("Washington", 250, "American History"),
        ]);
        assert_eq!(ents.len(), 3);
        assert_eq!(by_key(&ents, "washington").freq, 250);
        assert_eq!(by_key(&ents, "george washington").freq, 240);
        assert_eq!(by_key(&ents, "denzel washington").freq, 35);
    }

    #[test]
    fn place_stays_separate_from_person_by_category() {
        // "(Jack) London" licenses jack, but the category differs: London
        // the place vs. Jack London the author.
        let ents = resolve(&[
            fc("London", 296, "Geography & Exploration"),
            fc("Jack London", 70, "Literature & Language"),
            fc("(Jack) London", 17, "Literature & Language"),
        ]);
        assert_eq!(ents.len(), 2);
        assert_eq!(by_key(&ents, "london").freq, 296);
        assert_eq!(by_key(&ents, "jack london").freq, 87);
        // Beethoven: same category throughout, so the bare form (which used
        // to dominate by count) absorbs into the full name — the guard is
        // now category match, not count dominance.
        let ents = resolve(&[
            fc("(Ludwig van) Beethoven", 60, "Music & Performing Arts"),
            fc("Beethoven", 150, "Music & Performing Arts"),
            fc("Ludwig van Beethoven", 20, "Music & Performing Arts"),
        ]);
        assert_eq!(ents.len(), 1);
        let b = by_key(&ents, "ludwig van beethoven");
        assert_eq!(b.freq, 230);
        assert_eq!(b.display, "Ludwig van Beethoven");
        assert_eq!(b.forms[0], "Beethoven"); // forms sorted by count desc
    }

    #[test]
    fn lowercase_and_article_forms_are_never_absorbed() {
        let ents = resolve(&[
            fc("Shirley Temple", 60, "Film, TV & Pop Culture"),
            fc("(Shirley) Temple", 10, "Film, TV & Pop Culture"),
            fc("Temple", 8, "Film, TV & Pop Culture"),
            fc("temple", 12, "Philosophy, Religion & Society"),
            fc("the temple", 5, "Philosophy, Religion & Society"),
            fc("a temple", 3, "Philosophy, Religion & Society"),
        ]);
        assert_eq!(ents.len(), 2);
        let shirley = by_key(&ents, "shirley temple");
        assert_eq!(shirley.freq, 78);
        assert!(shirley.forms.iter().any(|f| f == "Temple"));
        let temple = by_key(&ents, "temple");
        assert_eq!(temple.freq, 20);
        assert_eq!(temple.display, "temple");
        assert_eq!(temple.forms, vec!["temple", "the temple", "a temple"]);
    }

    #[test]
    fn category_mismatch_blocks_absorption_even_when_rare() {
        let ents = resolve(&[
            fc("Hank Aaron", 90, "Sports & Games"),
            fc("(Hank) Aaron", 10, "Sports & Games"),
            fc("Aaron", 6, "Philosophy, Religion & Society"),
        ]);
        assert_eq!(ents.len(), 2);
        assert_eq!(by_key(&ents, "hank aaron").freq, 100);
        assert_eq!(by_key(&ents, "aaron").freq, 6);
    }

    #[test]
    fn same_category_absorbs_even_when_bare_dominates() {
        let ents = resolve(&[
            fc("Chopin", 72, "Music & Performing Arts"),
            fc("(Frederic) Chopin", 20, "Music & Performing Arts"),
            fc("Frederic Chopin", 15, "Music & Performing Arts"),
        ]);
        assert_eq!(ents.len(), 1);
        assert_eq!(ents[0].key, "frederic chopin");
        assert_eq!(ents[0].freq, 107);
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
        let ents = resolve(&[
            fc("(Claude) Debussy", 20, "Music & Performing Arts"),
            fc("Claude Debussy", 20, "Music & Performing Arts"),
            fc("Claude-Achille Debussy", 1, "Music & Performing Arts"),
            fc("Debussy", 3, "Music & Performing Arts"),
        ]);
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
