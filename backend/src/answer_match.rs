//! Jeopardy-style answer matching: parse accepted-answer conventions, normalize,
//! and (Task 3) grade typed responses with typo + phonetic forgiveness.

use deunicode::deunicode;

/// Lowercase, ASCII-fold, punctuation→space, collapse whitespace, drop ONE leading article.
pub fn normalize(s: &str) -> String {
    let folded = deunicode(s).to_lowercase();
    let cleaned: String = folded
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    let mut tokens: Vec<&str> = cleaned.split_whitespace().collect();
    if tokens.len() > 1 && matches!(tokens[0], "the" | "a" | "an") {
        tokens.remove(0);
    }
    tokens.join(" ")
}

/// Expand a raw accepted-answer string into acceptable literal variants
/// (J!Archive conventions). Variants are raw text; caller normalizes.
pub fn accepted_variants(raw: &str) -> Vec<String> {
    let cleaned = raw.replace("\\\"", "").replace('"', "");
    let mut bases: Vec<String> = Vec::new();

    // "(or X)" groups are standalone alternates; remaining parens are optional parts.
    let mut remainder = String::new();
    let mut alternates: Vec<String> = Vec::new();
    let mut rest = cleaned.as_str();
    while let Some(open) = rest.find('(') {
        let close = match rest[open..].find(')') {
            Some(c) => open + c,
            None => break, // unbalanced — treat rest as literal
        };
        let inner = rest[open + 1..close].trim();
        if let Some(alt) = inner.strip_prefix("or ") {
            alternates.push(alt.trim().to_string());
            remainder.push_str(&rest[..open]);
        } else {
            remainder.push_str(&rest[..close + 1]); // keep for optional expansion
        }
        rest = &rest[close + 1..];
    }
    remainder.push_str(rest);
    bases.push(remainder.trim().to_string());
    bases.extend(alternates);

    // Expand optional parens in each base: with and without. A paren glued to the
    // preceding word ("rappel(ing)") concatenates; a freestanding one is a word.
    let mut out: Vec<String> = Vec::new();
    for base in bases {
        let mut variants = vec![base.clone()];
        // Cap expansion: at most 3 paren groups → 8 variants.
        for _ in 0..3 {
            let mut next: Vec<String> = Vec::new();
            let mut changed = false;
            for v in &variants {
                if let (Some(open), true) = (v.find('('), v.contains(')')) {
                    let close = v[open..].find(')').unwrap() + open;
                    let inner = &v[open + 1..close];
                    let before = &v[..open];
                    let after = &v[close + 1..];
                    let glued = before.ends_with(|c: char| c.is_alphanumeric());
                    // with the parenthetical content
                    let with = if glued {
                        format!("{}{}{}", before, inner, after)
                    } else {
                        format!("{} {} {}", before.trim_end(), inner, after.trim_start())
                    };
                    // without it
                    let without = format!("{} {}", before.trim_end(), after.trim_start());
                    next.push(with.trim().to_string());
                    next.push(without.trim().to_string());
                    changed = true;
                } else {
                    next.push(v.clone());
                }
            }
            variants = next;
            if !changed {
                break;
            }
        }
        out.extend(variants);
    }

    out.retain(|v| !v.trim().is_empty());
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_case_punct_diacritics_articles() {
        assert_eq!(normalize("The U.S.S.R."), "u s s r");
        assert_eq!(normalize("Häagen-Dazs"), "haagen dazs");
        assert_eq!(normalize("  a  Möbius strip "), "mobius strip");
        assert_eq!(normalize("\"What A Wonderful World\""), "what a wonderful world");
    }

    #[test]
    fn variants_parenthetical_word_is_optional() {
        let v = accepted_variants("(Thomas) Cromwell");
        assert!(v.contains(&"Thomas Cromwell".to_string()));
        assert!(v.contains(&"Cromwell".to_string()));
    }

    #[test]
    fn variants_or_alternates_split() {
        let v = accepted_variants("the U.S.S.R. (or Soviet Union)");
        assert!(v.contains(&"the U.S.S.R.".to_string()));
        assert!(v.contains(&"Soviet Union".to_string()));
    }

    #[test]
    fn variants_inline_suffix() {
        let v = accepted_variants("rappel(ing)");
        assert!(v.contains(&"rappel".to_string()));
        assert!(v.contains(&"rappeling".to_string()));
    }

    #[test]
    fn variants_strip_escaped_quotes() {
        let v = accepted_variants("\\\"Sweet Dreams\\\"");
        assert!(v.contains(&"Sweet Dreams".to_string()));
    }

    #[test]
    fn variants_plain_answer_passes_through() {
        assert_eq!(accepted_variants("Bellerophon"), vec!["Bellerophon".to_string()]);
    }
}
