use axum::{
    extract::{Query, State},
    Json,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::routes::practice::{clue_json, ClueRow};
use crate::AppState;

const CLUE_COLS: &str =
    "id, question, answer, category, classifier_category, clue_value, round, air_date, notes";

/// Build the "clue matches the search + filters" SQL predicate.
///
/// `prefix` is the table alias prefix (`""` for a bare `jeopardy_questions`
/// query, `"jq."` when joined). `q_param` is the 1-based bind position of the
/// search string; `cat_param` the bind position of the classifier category when
/// filtering. `q` and category are ALWAYS bound (never interpolated); game-type
/// clauses are a fixed whitelist and the prefix is caller-controlled — so the
/// returned fragment carries no user-controlled string.
fn match_predicate(
    prefix: &str,
    q_param: usize,
    cat_param: Option<usize>,
    game_types: &[&str],
) -> String {
    let p = prefix;
    let mut c = vec![
        format!("{p}question IS NOT NULL"),
        format!("{p}answer IS NOT NULL"),
        format!("{p}classifier_category IS NOT NULL"),
        format!("{p}air_date IS NOT NULL"),
        format!("{p}archived = false"),
        format!("{p}search_tsv @@ websearch_to_tsquery('english', ${q_param})"),
    ];
    if let Some(ci) = cat_param {
        c.push(format!("{p}classifier_category = ${ci}"));
    }
    for gt in game_types {
        match *gt {
            "kids" | "Kids" => {
                c.push(format!("NOT ({p}notes ILIKE '%Kids%' OR {p}notes ILIKE '%Kid''s%')"))
            }
            "teen" | "Teen" => c.push(format!("NOT ({p}notes ILIKE '%Teen%')")),
            "college" | "College" => c.push(format!("NOT ({p}notes ILIKE '%College%')")),
            _ => {}
        }
    }
    c.join(" AND ")
}

pub async fn next(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let q = params.get("q").map(|s| s.trim()).unwrap_or("");
    if q.is_empty() {
        return Err(AppError::BadRequest("q (search query) is required".into()));
    }
    let category = params.get("category").map(|s| s.as_str()).unwrap_or("all");
    let use_category = category != "all";
    let game_types_str = params.get("gameTypes").map(|s| s.as_str()).unwrap_or("");
    let game_types: Vec<&str> = game_types_str
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    // Total matches (for the "N clues match" header). Binds: $1 = q, $2 = category?.
    let count_pred = match_predicate("", 1, if use_category { Some(2) } else { None }, &game_types);
    let count_sql = format!("SELECT COUNT(*) FROM jeopardy_questions WHERE {count_pred}");
    let mut cq = sqlx::query_scalar::<_, i64>(&count_sql).bind(q);
    if use_category {
        cq = cq.bind(category);
    }
    let match_count: i64 = cq.fetch_one(&state.pool).await?;

    // Tier-1: due matches (already-scheduled clues on this topic that are due).
    // Binds: $1 = user_id, $2 = q, $3 = category?.
    let due_pred = match_predicate("jq.", 2, if use_category { Some(3) } else { None }, &game_types);
    let due_join = format!(
        "FROM srs_cards sc JOIN jeopardy_questions jq ON jq.id = sc.question_id \
         WHERE sc.user_id = $1 AND sc.suspended = false AND sc.due <= now() AND {due_pred}"
    );
    let due_count_sql = format!("SELECT COUNT(*) {due_join}");
    let mut dcq = sqlx::query_scalar::<_, i64>(&due_count_sql).bind(user_id).bind(q);
    if use_category {
        dcq = dcq.bind(category);
    }
    let due_count: i64 = dcq.fetch_one(&state.pool).await?;

    // Tier-2: new matches (clue not yet in this user's SRS pool).
    // Binds: $1 = user_id, $2 = q, $3 = category?.
    let new_pred = match_predicate("jq.", 2, if use_category { Some(3) } else { None }, &game_types);
    let new_where = format!(
        "FROM jeopardy_questions jq \
         WHERE {new_pred} AND jq.id NOT IN (SELECT question_id FROM srs_cards WHERE user_id = $1)"
    );
    let new_count_sql = format!("SELECT COUNT(*) {new_where}");
    let mut ncq = sqlx::query_scalar::<_, i64>(&new_count_sql).bind(user_id).bind(q);
    if use_category {
        ncq = ncq.bind(category);
    }
    let new_count: i64 = ncq.fetch_one(&state.pool).await?;

    let remaining = due_count + new_count;

    // Serve tier-1 (soonest due) first.
    if due_count > 0 {
        let sql = format!(
            "SELECT {} {due_join} ORDER BY sc.due ASC LIMIT 1",
            prefixed_cols("jq.")
        );
        let mut q1 = sqlx::query_as::<_, ClueRow>(&sql).bind(user_id).bind(q);
        if use_category {
            q1 = q1.bind(category);
        }
        if let Some(row) = q1.fetch_optional(&state.pool).await? {
            crate::routes::practice::pregenerate_insight(&state, row.id);
            return Ok(Json(json!({
                "done": false, "isNew": false, "card": clue_json(row),
                "matchCount": match_count, "remaining": remaining,
            })));
        }
    }

    // Then a new match, recency-biased (same exponential offset as the Practice picker).
    if new_count > 0 {
        use rand::Rng;
        let r: f64 = rand::rng().random();
        let lambda = 3.5_f64;
        let normalized = (-(1.0 - r).ln() / lambda).min(1.0);
        let offset = (normalized * new_count as f64).floor() as i64;
        let sql = format!(
            "SELECT {} {new_where} ORDER BY jq.air_date DESC LIMIT 1 OFFSET {offset}",
            prefixed_cols("jq.")
        );
        let mut q2 = sqlx::query_as::<_, ClueRow>(&sql).bind(user_id).bind(q);
        if use_category {
            q2 = q2.bind(category);
        }
        if let Some(row) = q2.fetch_optional(&state.pool).await? {
            crate::routes::practice::pregenerate_insight(&state, row.id);
            return Ok(Json(json!({
                "done": false, "isNew": true, "card": clue_json(row),
                "matchCount": match_count, "remaining": remaining,
            })));
        }
    }

    Ok(Json(json!({ "done": true, "matchCount": match_count, "remaining": 0 })))
}

/// The clue column list, each prefixed with the table alias.
fn prefixed_cols(prefix: &str) -> String {
    CLUE_COLS
        .split(", ")
        .map(|col| format!("{prefix}{col}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::match_predicate;

    #[test]
    fn base_predicate_binds_query_and_has_no_category() {
        let p = match_predicate("", 1, None, &[]);
        assert!(p.contains("search_tsv @@ websearch_to_tsquery('english', $1)"));
        assert!(p.contains("archived = false"));
        assert!(!p.contains("classifier_category ="));
        // q is bound, never interpolated
        assert!(!p.contains("websearch_to_tsquery('english', '"));
    }

    #[test]
    fn prefixed_predicate_with_category_uses_given_bind_positions() {
        let p = match_predicate("jq.", 2, Some(3), &[]);
        assert!(p.contains("jq.search_tsv @@ websearch_to_tsquery('english', $2)"));
        assert!(p.contains("jq.classifier_category = $3"));
        assert!(p.contains("jq.archived = false"));
    }

    #[test]
    fn game_types_expand_to_whitelisted_clauses_only() {
        let p = match_predicate("", 1, None, &["kids", "Teen", "college", "bogus"]);
        assert!(p.contains("NOT (notes ILIKE '%Kids%'"));
        assert!(p.contains("NOT (notes ILIKE '%Teen%')"));
        assert!(p.contains("NOT (notes ILIKE '%College%')"));
        // unknown game types contribute nothing
        assert!(!p.to_lowercase().contains("bogus"));
    }
}
