use axum::extract::{Path, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::answer_match::is_correct;
use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

pub const TEST_SIZE: i64 = 50;
pub const PASS_LINE: i64 = 35;

/// Largest-remainder apportionment of `seats` across categories weighted by pool size.
pub fn apportion(dist: &[(String, i64)], seats: i64) -> Vec<(String, i64)> {
    let total: i64 = dist.iter().map(|(_, n)| n).sum();
    if total == 0 || dist.is_empty() {
        return vec![];
    }
    let mut rows: Vec<(String, i64, f64)> = dist
        .iter()
        .map(|(c, n)| {
            let exact = seats as f64 * *n as f64 / total as f64;
            (c.clone(), exact.floor() as i64, exact - exact.floor())
        })
        .collect();
    let mut assigned: i64 = rows.iter().map(|(_, f, _)| f).sum();
    rows.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    let len = rows.len();
    let mut i = 0;
    while assigned < seats {
        let idx = i % len;
        rows[idx].1 += 1;
        assigned += 1;
        i += 1;
    }
    rows.into_iter().map(|(c, s, _)| (c, s)).collect()
}

const MIDBAND: &str = "((jq.round = 1 AND jq.clue_value BETWEEN 600 AND 1000) \
                       OR (jq.round = 2 AND jq.clue_value BETWEEN 800 AND 1200))";

async fn active_test(
    state: &Arc<AppState>,
    user_id: i32,
) -> Result<Option<(i32, Vec<i32>, i32)>, AppError> {
    let row: Option<(i32, Vec<i32>, i32)> = sqlx::query_as(
        "SELECT id, question_ids, current_index FROM mock_tests
         WHERE user_id = $1 AND completed_at IS NULL
         ORDER BY started_at DESC LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?;
    Ok(row)
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;

    if let Some((id, _, idx)) = active_test(&state, user_id).await? {
        return Ok(Json(json!({ "testId": id, "resumed": true, "position": idx, "total": TEST_SIZE })));
    }

    // Eligible-pool distribution per category (unseen, mid-band).
    let dist_sql = format!(
        "SELECT jq.classifier_category, COUNT(*)::bigint
         FROM jeopardy_questions jq
         WHERE jq.archived = false AND jq.question IS NOT NULL AND jq.answer IS NOT NULL
           AND jq.classifier_category IS NOT NULL AND {MIDBAND}
           AND NOT EXISTS (SELECT 1 FROM question_attempts qa WHERE qa.user_id = $1 AND qa.question_id = jq.id)
           AND NOT EXISTS (SELECT 1 FROM srs_cards sc WHERE sc.user_id = $1 AND sc.question_id = jq.id)
         GROUP BY jq.classifier_category"
    );
    let dist: Vec<(String, i64)> = sqlx::query_as(&dist_sql).bind(user_id).fetch_all(&state.pool).await?;
    if dist.iter().map(|(_, n)| n).sum::<i64>() < TEST_SIZE {
        return Err(AppError::BadRequest("Not enough unseen clues for a mock test".into()));
    }

    let quotas = apportion(&dist, TEST_SIZE);
    let mut ids: Vec<i32> = Vec::with_capacity(TEST_SIZE as usize);
    for (cat, seats) in &quotas {
        if *seats == 0 { continue; }
        let sel_sql = format!(
            "SELECT jq.id FROM jeopardy_questions jq
             WHERE jq.archived = false AND jq.question IS NOT NULL AND jq.answer IS NOT NULL
               AND jq.classifier_category = $2 AND {MIDBAND}
               AND NOT EXISTS (SELECT 1 FROM question_attempts qa WHERE qa.user_id = $1 AND qa.question_id = jq.id)
               AND NOT EXISTS (SELECT 1 FROM srs_cards sc WHERE sc.user_id = $1 AND sc.question_id = jq.id)
             ORDER BY random() LIMIT $3"
        );
        let picked: Vec<(i32,)> = sqlx::query_as(&sel_sql)
            .bind(user_id).bind(cat).bind(seats)
            .fetch_all(&state.pool).await?;
        ids.extend(picked.into_iter().map(|(i,)| i));
    }
    // Shortfall (a category quota exceeded its pool): top up from any category.
    if (ids.len() as i64) < TEST_SIZE {
        let need = TEST_SIZE - ids.len() as i64;
        tracing::warn!("mock test shortfall: borrowing {} clues across categories", need);
        let fill_sql = format!(
            "SELECT jq.id FROM jeopardy_questions jq
             WHERE jq.archived = false AND jq.question IS NOT NULL AND jq.answer IS NOT NULL
               AND jq.classifier_category IS NOT NULL AND {MIDBAND}
               AND jq.id <> ALL($2)
               AND NOT EXISTS (SELECT 1 FROM question_attempts qa WHERE qa.user_id = $1 AND qa.question_id = jq.id)
               AND NOT EXISTS (SELECT 1 FROM srs_cards sc WHERE sc.user_id = $1 AND sc.question_id = jq.id)
             ORDER BY random() LIMIT $3"
        );
        let extra: Vec<(i32,)> = sqlx::query_as(&fill_sql)
            .bind(user_id).bind(&ids).bind(need)
            .fetch_all(&state.pool).await?;
        ids.extend(extra.into_iter().map(|(i,)| i));
    }

    use rand::seq::SliceRandom;
    ids.shuffle(&mut rand::rng());

    // One quiz_sessions row anchors the question_attempts FK for this test.
    let (session_id,): (i32,) = sqlx::query_as(
        "INSERT INTO quiz_sessions (user_id, is_review_session) VALUES ($1, false) RETURNING id",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;

    let (test_id,): (i32,) = sqlx::query_as(
        "INSERT INTO mock_tests (user_id, session_id, question_ids) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(user_id)
    .bind(session_id)
    .bind(&ids)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(json!({ "testId": test_id, "resumed": false, "position": 0, "total": TEST_SIZE })))
}

pub async fn current(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let (test_id, ids, idx) = active_test(&state, auth.user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("No active mock test".into()))?;
    let qid = ids[idx as usize];
    // `answer` is the clue text; `question` (the accepted response) is NOT sent mid-test.
    let (category, text): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT category, answer FROM jeopardy_questions WHERE id = $1",
    )
    .bind(qid)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(json!({
        "testId": test_id, "position": idx, "total": TEST_SIZE,
        "clue": { "id": qid, "category": category, "text": text },
    })))
}

#[cfg(test)]
mod tests {
    use super::apportion;

    fn seats(v: &[(String, i64)], name: &str) -> i64 {
        v.iter().find(|(c, _)| c == name).map(|(_, s)| *s).unwrap_or(0)
    }

    #[test]
    fn apportion_sums_to_seats_and_tracks_proportion() {
        let dist = vec![
            ("History".to_string(), 30000_i64),
            ("Science".to_string(), 24000),
            ("Math".to_string(), 2500),
        ];
        let q = apportion(&dist, 50);
        assert_eq!(q.iter().map(|(_, s)| s).sum::<i64>(), 50);
        assert!(seats(&q, "History") > seats(&q, "Science"));
        assert!(seats(&q, "Math") >= 1); // largest remainder keeps small cats alive
    }

    #[test]
    fn apportion_handles_empty_and_zero() {
        assert!(apportion(&[], 50).is_empty());
        let q = apportion(&[("A".to_string(), 10)], 50);
        assert_eq!(q, vec![("A".to_string(), 50)]);
    }
}
