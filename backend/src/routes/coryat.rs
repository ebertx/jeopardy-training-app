use axum::{
    extract::{Path, State},
    Json,
};
use chrono::NaiveDate;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::models::coryat::{CoryatAnswerRequest, CoryatGame};
use crate::models::question::Question;
use crate::AppState;

const DOUBLE_VALUE_DATE: &str = "2001-11-26";
const J_VALUES: [i32; 5] = [200, 400, 600, 800, 1000];
const DJ_VALUES: [i32; 5] = [400, 800, 1200, 1600, 2000];

fn normalize_clue_value(value: i32, air_date: &NaiveDate) -> i32 {
    let cutoff = NaiveDate::parse_from_str(DOUBLE_VALUE_DATE, "%Y-%m-%d").unwrap();
    if *air_date < cutoff && value < 1000 {
        value * 2
    } else {
        value
    }
}

async fn find_question_for_cell(
    pool: &sqlx::PgPool,
    category: &str,
    value: i32,
    round: i32,
) -> Option<Question> {
    // Try 1: exact category + clue_value + round
    let q = sqlx::query_as::<_, Question>(
        "SELECT id, question, answer, category, classifier_category, clue_value, round, air_date, notes \
         FROM jeopardy_questions \
         WHERE category = $1 AND archived = false AND clue_value = $2 AND (round = $3 OR round IS NULL) \
         ORDER BY air_date DESC NULLS LAST LIMIT 1",
    )
    .bind(category)
    .bind(value)
    .bind(round)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    if q.is_some() {
        return q;
    }

    // Fallback 1: category + round, any value
    let q = sqlx::query_as::<_, Question>(
        "SELECT id, question, answer, category, classifier_category, clue_value, round, air_date, notes \
         FROM jeopardy_questions \
         WHERE category = $1 AND archived = false AND round = $2 AND clue_value IS NOT NULL \
         ORDER BY air_date DESC NULLS LAST LIMIT 1",
    )
    .bind(category)
    .bind(round)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    if q.is_some() {
        return q;
    }

    // Fallback 2: just category
    sqlx::query_as::<_, Question>(
        "SELECT id, question, answer, category, classifier_category, clue_value, round, air_date, notes \
         FROM jeopardy_questions \
         WHERE category = $1 AND archived = false \
         ORDER BY air_date DESC NULLS LAST LIMIT 1",
    )
    .bind(category)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;

    // Step 1: Get top 100 categories by question count
    let top_cats: Vec<(String,)> = sqlx::query_as(
        "SELECT category FROM jeopardy_questions \
         WHERE category IS NOT NULL AND archived = false \
           AND air_date IS NOT NULL AND classifier_category IS NOT NULL \
         GROUP BY category ORDER BY COUNT(*) DESC LIMIT 100",
    )
    .fetch_all(&state.pool)
    .await?;

    let mut all_cats: Vec<String> = top_cats.into_iter().map(|(c,)| c).collect();

    if all_cats.len() < 12 {
        return Err(AppError::Internal("Not enough categories".to_string()));
    }

    // Step 2: Shuffle and pick 6 for J and 6 different for DJ
    // Use rng in a sync block so it's dropped before any .await
    let (j_cats, dj_cats) = {
        use rand::seq::SliceRandom;
        let mut rng = rand::rng();
        all_cats.shuffle(&mut rng);
        let j = all_cats[0..6].to_vec();
        let dj = all_cats[6..12].to_vec();
        (j, dj)
    };

    // Step 3: Build questions for each round
    // Jeopardy round (round = 1), 6 cols x 5 rows
    let mut j_questions: Vec<Value> = Vec::new();
    let mut j_question_ids: Vec<Option<i64>> = Vec::new();

    for (col, cat) in j_cats.iter().enumerate() {
        for (row, &value) in J_VALUES.iter().enumerate() {
            let q = find_question_for_cell(&state.pool, cat, value, 1).await;
            let question_id: Option<i64> = q.as_ref().map(|q| q.id as i64);

            // Normalize value based on air_date
            let display_value = match &q {
                Some(q) if q.air_date.is_some() => {
                    normalize_clue_value(value, q.air_date.as_ref().unwrap())
                }
                _ => value,
            };

            j_question_ids.push(question_id);
            j_questions.push(json!({
                "col": col,
                "row": row,
                "question_id": question_id,
                "value": display_value,
                "answered": null,
                "daily_double": false,
            }));
        }
    }

    // Double Jeopardy round (round = 2), 6 cols x 5 rows
    let mut dj_questions: Vec<Value> = Vec::new();
    let mut dj_question_ids: Vec<Option<i64>> = Vec::new();

    for (col, cat) in dj_cats.iter().enumerate() {
        for (row, &value) in DJ_VALUES.iter().enumerate() {
            let q = find_question_for_cell(&state.pool, cat, value, 2).await;
            let question_id: Option<i64> = q.as_ref().map(|q| q.id as i64);

            let display_value = match &q {
                Some(q) if q.air_date.is_some() => {
                    normalize_clue_value(value, q.air_date.as_ref().unwrap())
                }
                _ => value,
            };

            dj_question_ids.push(question_id);
            dj_questions.push(json!({
                "col": col,
                "row": row,
                "question_id": question_id,
                "value": display_value,
                "answered": null,
                "daily_double": false,
            }));
        }
    }

    // Step 5: Assign Daily Doubles
    // Use rng in sync block, dropped before next .await
    {
        use rand::prelude::IndexedRandom;
        use rand::seq::SliceRandom;
        let mut rng = rand::rng();

        // J: 1 DD from non-null cells
        let j_valid_indices: Vec<usize> = j_question_ids
            .iter()
            .enumerate()
            .filter(|(_, id)| id.is_some())
            .map(|(i, _)| i)
            .collect();

        if let Some(&dd_idx) = j_valid_indices.choose(&mut rng) {
            if let Some(cell) = j_questions.get_mut(dd_idx) {
                cell["daily_double"] = json!(true);
            }
        }

        // DJ: 2 DDs from non-null cells
        let mut dj_valid_indices: Vec<usize> = dj_question_ids
            .iter()
            .enumerate()
            .filter(|(_, id)| id.is_some())
            .map(|(i, _)| i)
            .collect();

        dj_valid_indices.shuffle(&mut rng);
        for &dd_idx in dj_valid_indices.iter().take(2) {
            if let Some(cell) = dj_questions.get_mut(dd_idx) {
                cell["daily_double"] = json!(true);
            }
        }
    }

    // Step 6: Find Final Jeopardy question
    let fj_question = sqlx::query_as::<_, Question>(
        "SELECT id, question, answer, category, classifier_category, clue_value, round, air_date, notes \
         FROM jeopardy_questions \
         WHERE round = 3 AND question IS NOT NULL AND answer IS NOT NULL AND archived = false \
         ORDER BY air_date DESC NULLS LAST LIMIT 1",
    )
    .fetch_optional(&state.pool)
    .await?;

    let fj_question = match fj_question {
        Some(q) => q,
        None => {
            // Fallback: any recent non-archived question
            sqlx::query_as::<_, Question>(
                "SELECT id, question, answer, category, classifier_category, clue_value, round, air_date, notes \
                 FROM jeopardy_questions WHERE archived = false \
                 ORDER BY air_date DESC NULLS LAST LIMIT 1",
            )
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| AppError::NotFound("No questions available".to_string()))?
        }
    };

    let fj_category = fj_question.category.clone().unwrap_or_else(|| "Final Jeopardy".to_string());
    let fj_question_id = fj_question.id as i64;

    // Step 7: Build game_board JSON
    let game_board = json!({
        "rounds": {
            "jeopardy": {
                "categories": j_cats,
                "questions": j_questions,
            },
            "double_jeopardy": {
                "categories": dj_cats,
                "questions": dj_questions,
            },
            "final_jeopardy": {
                "category": fj_category,
                "question_id": fj_question_id,
                "answered": null,
            }
        }
    });

    // Step 8: Insert into DB
    let row: (i32,) = sqlx::query_as(
        "INSERT INTO coryat_games (user_id, game_board, current_round, questions_answered, jeopardy_score, double_j_score) \
         VALUES ($1, $2, 1, 0, 0, 0) RETURNING id",
    )
    .bind(user_id)
    .bind(&game_board)
    .fetch_one(&state.pool)
    .await?;

    let game_id = row.0;

    Ok(Json(json!({
        "success": true,
        "game_id": game_id,
    })))
}

fn shape_game_response(game: &crate::models::coryat::CoryatGame) -> Value {
    let board = &game.game_board;
    let rounds_obj = board.get("rounds");
    let jeopardy = rounds_obj.and_then(|r| r.get("jeopardy")).cloned().unwrap_or(json!({}));
    let double_j = rounds_obj.and_then(|r| r.get("double_jeopardy")).cloned().unwrap_or(json!({}));
    let final_j = rounds_obj.and_then(|r| r.get("final_jeopardy")).cloned().unwrap_or(json!({}));
    json!({
        "id": game.id,
        "rounds": [
            {
                "round": "jeopardy",
                "categories": jeopardy.get("categories").cloned().unwrap_or(json!([])),
                "questions": jeopardy.get("questions").cloned().unwrap_or(json!([])),
            },
            {
                "round": "double_jeopardy",
                "categories": double_j.get("categories").cloned().unwrap_or(json!([])),
                "questions": double_j.get("questions").cloned().unwrap_or(json!([])),
            },
        ],
        "final_jeopardy": final_j,
        "started_at": game.started_at,
        "completed_at": game.completed_at,
        "jeopardy_score": game.jeopardy_score,
        "double_jeopardy_score": game.double_j_score,
        "final_score": game.final_score,
        "total_score": game.jeopardy_score + game.double_j_score,
        "current_round": game.current_round,
        "questions_answered": game.questions_answered,
    })
}

pub async fn get_game(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<i32>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;

    let game = sqlx::query_as::<_, CoryatGame>(
        "SELECT * FROM coryat_games WHERE id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("Game not found".to_string()))?;

    Ok(Json(shape_game_response(&game)))
}

pub async fn answer(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<i32>,
    Json(body): Json<CoryatAnswerRequest>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;

    let game = sqlx::query_as::<_, CoryatGame>(
        "SELECT * FROM coryat_games WHERE id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("Game not found".to_string()))?;

    // Validate: game not completed
    if game.completed_at.is_some() {
        return Err(AppError::BadRequest("Game already completed".to_string()));
    }

    // Find the question cell in the game board
    let round_key = match body.round.as_str() {
        "jeopardy" => "jeopardy",
        "double_jeopardy" => "double_jeopardy",
        other => return Err(AppError::BadRequest(format!("Invalid round: {}", other))),
    };

    let questions = game
        .game_board
        .get("rounds")
        .and_then(|r| r.get(round_key))
        .and_then(|r| r.get("questions"))
        .and_then(|q| q.as_array())
        .ok_or_else(|| AppError::BadRequest("Invalid game board structure".to_string()))?;

    // Find matching cell
    let cell_idx = questions
        .iter()
        .position(|q| {
            q.get("col").and_then(|v| v.as_i64()) == Some(body.col as i64)
                && q.get("row").and_then(|v| v.as_i64()) == Some(body.row as i64)
        })
        .ok_or_else(|| AppError::BadRequest("Cell not found".to_string()))?;

    let cell = &questions[cell_idx];

    // Validate: not already answered
    if cell.get("answered").and_then(|v| v.as_str()).is_some() {
        return Err(AppError::BadRequest("Question already answered".to_string()));
    }

    // Validate: question_id not null
    let question_id = cell
        .get("question_id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| AppError::BadRequest("No question assigned to this cell".to_string()))?;

    let _ = question_id; // used for validation

    let value = cell
        .get("value")
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;

    // Calculate score change
    let score_change = match body.result.as_str() {
        "correct" => value,
        "incorrect" => -value,
        _ => 0, // "pass" or anything else
    };

    // Clone and mutate the game board
    let mut new_board = game.game_board.clone();
    let cell_mut = new_board
        .get_mut("rounds")
        .and_then(|r| r.get_mut(round_key))
        .and_then(|r| r.get_mut("questions"))
        .and_then(|q| q.as_array_mut())
        .and_then(|arr| arr.get_mut(cell_idx))
        .ok_or_else(|| AppError::Internal("Failed to mutate game board".to_string()))?;

    cell_mut["answered"] = json!(body.result);

    // Update scores
    let new_j_score;
    let new_dj_score;

    if body.round == "jeopardy" {
        new_j_score = game.jeopardy_score + score_change;
        new_dj_score = game.double_j_score;
    } else {
        new_j_score = game.jeopardy_score;
        new_dj_score = game.double_j_score + score_change;
    }

    let new_questions_answered = game.questions_answered + 1;

    sqlx::query(
        "UPDATE coryat_games SET game_board = $1, jeopardy_score = $2, double_j_score = $3, questions_answered = $4 \
         WHERE id = $5 AND user_id = $6",
    )
    .bind(&new_board)
    .bind(new_j_score)
    .bind(new_dj_score)
    .bind(new_questions_answered)
    .bind(id)
    .bind(user_id)
    .execute(&state.pool)
    .await?;

    // Count total non-null questions
    let total_questions = count_total_questions(&game.game_board);
    let questions_remaining = total_questions - new_questions_answered;

    let current_round_score = if body.round == "jeopardy" {
        new_j_score
    } else {
        new_dj_score
    };

    Ok(Json(json!({
        "success": true,
        "score_change": score_change,
        "current_round_score": current_round_score,
        "total_score": new_j_score + new_dj_score,
        "questions_remaining": questions_remaining,
        "questions_answered": new_questions_answered,
        "jeopardy_score": new_j_score,
        "double_jeopardy_score": new_dj_score,
    })))
}

fn count_total_questions(game_board: &Value) -> i32 {
    let mut count = 0i32;

    for round_key in &["jeopardy", "double_jeopardy"] {
        if let Some(questions) = game_board
            .get("rounds")
            .and_then(|r| r.get(*round_key))
            .and_then(|r| r.get("questions"))
            .and_then(|q| q.as_array())
        {
            for q in questions {
                if q.get("question_id").and_then(|v| v.as_i64()).is_some() {
                    count += 1;
                }
            }
        }
    }

    count
}

pub async fn complete(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<i32>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;

    let row: Option<(i32,)> = sqlx::query_as(
        "UPDATE coryat_games SET completed_at = NOW(), final_score = jeopardy_score + double_j_score \
         WHERE id = $1 AND user_id = $2 \
         RETURNING final_score",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?;

    let (final_score,) = row.ok_or_else(|| AppError::NotFound("Game not found".to_string()))?;

    Ok(Json(json!({
        "success": true,
        "final_score": final_score,
    })))
}

pub async fn history(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;

    let games = sqlx::query_as::<_, CoryatGame>(
        "SELECT * FROM coryat_games WHERE user_id = $1 AND completed_at IS NOT NULL \
         ORDER BY completed_at DESC",
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await?;

    let summaries: Vec<Value> = games
        .iter()
        .map(|g| {
            json!({
                "id": g.id,
                "started_at": g.started_at,
                "completed_at": g.completed_at,
                "jeopardy_score": g.jeopardy_score,
                "double_jeopardy_score": g.double_j_score,
                "final_score": g.final_score,
            })
        })
        .collect();

    Ok(Json(json!(summaries)))
}
