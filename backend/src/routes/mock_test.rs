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

// Weighted sampling via the exponential race: the row minimizing -ln(u)/w is a
// draw with probability proportional to w. answer_freq >= 1 keeps the divisor
// positive; air_date is NOT NULL corpus-wide.
const CANON_ORDER: &str = "-ln(random()) / ln(1 + jq.answer_freq)";
const RECENCY_ORDER: &str =
    "-ln(random()) * exp(0.11552 * EXTRACT(EPOCH FROM (now() - jq.air_date)) / 31557600.0)";

async fn draw_category(
    state: &Arc<AppState>,
    user_id: i32,
    category: &str,
    seats: i64,
    order_expr: &str,
    exclude: &[i32],
) -> Result<Vec<i32>, AppError> {
    if seats <= 0 {
        return Ok(vec![]);
    }
    let sql = format!(
        "SELECT jq.id FROM jeopardy_questions jq
         WHERE jq.archived = false AND jq.question IS NOT NULL AND jq.answer IS NOT NULL
           AND jq.classifier_category = $2 AND {MIDBAND}
           AND jq.id <> ALL($4)
           AND NOT EXISTS (SELECT 1 FROM question_attempts qa WHERE qa.user_id = $1 AND qa.question_id = jq.id)
           AND NOT EXISTS (SELECT 1 FROM srs_cards sc WHERE sc.user_id = $1 AND sc.question_id = jq.id)
         ORDER BY {order_expr} LIMIT $3"
    );
    let picked: Vec<(i32,)> = sqlx::query_as(&sql)
        .bind(user_id)
        .bind(category)
        .bind(seats)
        .bind(exclude.to_vec())
        .fetch_all(&state.pool)
        .await?;
    Ok(picked.into_iter().map(|(i,)| i).collect())
}

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

    let available: Vec<String> = dist.iter().map(|(c, _)| c.clone()).collect();
    let weights = crate::blend::target_weights(&available);
    let quotas = apportion(&weights, TEST_SIZE);
    let mut ids: Vec<i32> = Vec::with_capacity(TEST_SIZE as usize);
    for (cat, seats) in &quotas {
        if *seats == 0 {
            continue;
        }
        match crate::blend::sampling_kind(cat) {
            crate::blend::SamplingKind::Canon => {
                let picked = draw_category(&state, user_id, cat, *seats, CANON_ORDER, &ids).await?;
                ids.extend(picked);
            }
            crate::blend::SamplingKind::Recency => {
                let picked = draw_category(&state, user_id, cat, *seats, RECENCY_ORDER, &ids).await?;
                ids.extend(picked);
            }
            crate::blend::SamplingKind::Split => {
                let (canon_seats, recency_seats) = crate::blend::split_seats(*seats);
                let picked = draw_category(&state, user_id, cat, canon_seats, CANON_ORDER, &ids).await?;
                ids.extend(picked);
                let picked = draw_category(&state, user_id, cat, recency_seats, RECENCY_ORDER, &ids).await?;
                ids.extend(picked);
            }
        }
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
    if idx as i64 >= TEST_SIZE || idx as usize >= ids.len() {
        return Err(AppError::NotFound("No active mock test".into()));
    }
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerBody {
    pub position: i32,
    pub typed_answer: String,
    pub response_ms: i32,
}

pub async fn answer(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<AnswerBody>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let (test_id, ids, idx) = active_test(&state, user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("No active mock test".into()))?;
    if idx as i64 >= TEST_SIZE || idx as usize >= ids.len() {
        return Err(AppError::NotFound("No active mock test".into()));
    }
    if body.position != idx {
        return Err(AppError::Conflict(format!("Expected position {idx}")));
    }
    let qid = ids[idx as usize];
    let (accepted,): (Option<String>,) =
        sqlx::query_as("SELECT question FROM jeopardy_questions WHERE id = $1")
            .bind(qid)
            .fetch_one(&state.pool)
            .await?;
    let correct = accepted
        .as_deref()
        .map(|a| is_correct(&body.typed_answer, a))
        .unwrap_or(false);

    sqlx::query(
        "INSERT INTO mock_test_answers
           (mock_test_id, question_id, position, typed_answer, response_ms, auto_correct, final_correct)
         VALUES ($1, $2, $3, $4, $5, $6, $6)
         ON CONFLICT (mock_test_id, position) DO NOTHING",
    )
    .bind(test_id).bind(qid).bind(idx)
    .bind(&body.typed_answer).bind(body.response_ms).bind(correct)
    .execute(&state.pool)
    .await?;

    let (session_id,): (i32,) =
        sqlx::query_as("SELECT session_id FROM mock_tests WHERE id = $1")
            .bind(test_id)
            .fetch_one(&state.pool)
            .await?;
    sqlx::query(
        "INSERT INTO question_attempts (session_id, question_id, user_id, correct, attempt_kind)
         VALUES ($1, $2, $3, $4, 'mock')",
    )
    .bind(session_id).bind(qid).bind(user_id).bind(correct)
    .execute(&state.pool)
    .await?;

    // Atomic: bump current_index and, if this was the last clue, set completed_at/score
    // in the same statement so a crash between two separate UPDATEs can never leave
    // current_index == TEST_SIZE on an uncompleted test.
    let next_idx = idx + 1;
    let (score,): (Option<i32>,) = sqlx::query_as(
        "UPDATE mock_tests SET current_index = $2,
           completed_at = CASE WHEN $2 >= $3 THEN now() ELSE completed_at END,
           score = CASE WHEN $2 >= $3 THEN
             (SELECT COUNT(*) FILTER (WHERE final_correct) FROM mock_test_answers WHERE mock_test_id = $1)::int
           ELSE score END
         WHERE id = $1
         RETURNING score",
    )
    .bind(test_id).bind(next_idx).bind(TEST_SIZE as i32)
    .fetch_one(&state.pool)
    .await?;
    if next_idx as i64 >= TEST_SIZE {
        sqlx::query("UPDATE quiz_sessions SET completed_at = now() WHERE id = $1")
            .bind(session_id)
            .execute(&state.pool)
            .await?;
        return Ok(Json(json!({ "completed": true, "position": next_idx, "total": TEST_SIZE, "score": score })));
    }
    Ok(Json(json!({ "completed": false, "position": next_idx, "total": TEST_SIZE })))
}

/// Loads a completed, owned test or errors.
async fn owned_completed_test(
    state: &Arc<AppState>,
    user_id: i32,
    test_id: i32,
) -> Result<(i32, Option<i32>), AppError> {
    let row: Option<(i32, Option<i32>, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT session_id, score, completed_at FROM mock_tests WHERE id = $1 AND user_id = $2",
    )
    .bind(test_id).bind(user_id)
    .fetch_optional(&state.pool)
    .await?;
    match row {
        None => Err(AppError::NotFound("Mock test not found".into())),
        Some((_, _, None)) => Err(AppError::BadRequest("Mock test not completed".into())),
        Some((session_id, score, Some(_))) => Ok((session_id, score)),
    }
}

pub async fn results(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(test_id): Path<i32>,
) -> Result<Json<Value>, AppError> {
    let _ = owned_completed_test(&state, auth.user_id, test_id).await?;
    #[derive(sqlx::FromRow)]
    struct Row {
        position: i32,
        typed_answer: String,
        response_ms: i32,
        auto_correct: bool,
        overridden: bool,
        final_correct: bool,
        clue: Option<String>,
        accepted: Option<String>,
        category: Option<String>,
    }
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT mta.position, mta.typed_answer, mta.response_ms, mta.auto_correct,
                mta.overridden, mta.final_correct,
                jq.answer AS clue, jq.question AS accepted, jq.category
         FROM mock_test_answers mta
         JOIN jeopardy_questions jq ON jq.id = mta.question_id
         WHERE mta.mock_test_id = $1
         ORDER BY mta.position",
    )
    .bind(test_id)
    .fetch_all(&state.pool)
    .await?;

    let (score, completed_at): (Option<i32>, Option<DateTime<Utc>>) =
        sqlx::query_as("SELECT score, completed_at FROM mock_tests WHERE id = $1")
            .bind(test_id)
            .fetch_one(&state.pool)
            .await?;

    let answers: Vec<Value> = rows.into_iter().map(|r| json!({
        "position": r.position, "clue": r.clue, "category": r.category,
        "accepted": r.accepted, "typed": r.typed_answer, "responseMs": r.response_ms,
        "autoCorrect": r.auto_correct, "overridden": r.overridden, "finalCorrect": r.final_correct,
    })).collect();

    Ok(Json(json!({ "score": score, "passLine": PASS_LINE, "completedAt": completed_at, "answers": answers })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverrideBody {
    pub position: i32,
    pub correct: bool,
}

pub async fn override_verdict(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(test_id): Path<i32>,
    Json(body): Json<OverrideBody>,
) -> Result<Json<Value>, AppError> {
    let (session_id, _) = owned_completed_test(&state, auth.user_id, test_id).await?;

    let qid: Option<i32> = sqlx::query_scalar(
        "UPDATE mock_test_answers SET overridden = true, final_correct = $3
         WHERE mock_test_id = $1 AND position = $2 RETURNING question_id",
    )
    .bind(test_id).bind(body.position).bind(body.correct)
    .fetch_optional(&state.pool)
    .await?;
    let qid = qid.ok_or_else(|| AppError::NotFound("No answer at that position".into()))?;

    sqlx::query(
        "UPDATE question_attempts SET correct = $4
         WHERE session_id = $1 AND question_id = $2 AND user_id = $3 AND attempt_kind = 'mock'",
    )
    .bind(session_id).bind(qid).bind(auth.user_id).bind(body.correct)
    .execute(&state.pool)
    .await?;

    let (score,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FILTER (WHERE final_correct) FROM mock_test_answers WHERE mock_test_id = $1",
    )
    .bind(test_id)
    .fetch_one(&state.pool)
    .await?;
    sqlx::query("UPDATE mock_tests SET score = $2 WHERE id = $1")
        .bind(test_id).bind(score as i32)
        .execute(&state.pool)
        .await?;

    Ok(Json(json!({ "score": score })))
}

pub async fn add_misses_to_srs(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(test_id): Path<i32>,
) -> Result<Json<Value>, AppError> {
    let _ = owned_completed_test(&state, auth.user_id, test_id).await?;
    let added: i64 = sqlx::query_scalar(
        "WITH ins AS (
           INSERT INTO srs_cards (user_id, question_id)
           SELECT $2, mta.question_id
           FROM mock_test_answers mta
           WHERE mta.mock_test_id = $1 AND mta.final_correct = false
           ON CONFLICT (user_id, question_id) DO NOTHING
           RETURNING 1
         ) SELECT COUNT(*) FROM ins",
    )
    .bind(test_id).bind(auth.user_id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(json!({ "added": added })))
}

pub async fn history(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let rows: Vec<(i32, Option<DateTime<Utc>>, Option<i32>)> = sqlx::query_as(
        "SELECT id, completed_at, score FROM mock_tests
         WHERE user_id = $1 AND completed_at IS NOT NULL
         ORDER BY completed_at DESC",
    )
    .bind(auth.user_id)
    .fetch_all(&state.pool)
    .await?;
    let best = rows.iter().filter_map(|(_, _, s)| *s).max();
    let tests: Vec<Value> = rows.into_iter()
        .map(|(id, at, s)| json!({ "id": id, "completedAt": at, "score": s }))
        .collect();
    Ok(Json(json!({ "tests": tests, "best": best, "passLine": PASS_LINE })))
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
