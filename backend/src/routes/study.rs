use axum::{
    extract::State,
    Json,
};
use chrono::{Duration, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::models::study::StudyRecommendation;
use crate::AppState;

const SYSTEM_PROMPT: &str = r#"You are a Jeopardy! training analyst in the style of Ken Jennings. Analyze the player's missed clues and provide targeted study recommendations.

Rules:
- Output ONLY valid JSON (no Markdown, no comments, no trailing commas)
- Group findings into 3-6 crisp topics (not slivers)
- Be concrete and Jeopardy!-aware (wordplay, eponyms, before-&-after, homophones)
- Sources must actually exist and be high-yield
- Provide at least one free/open source per topic
- Wikipedia links: 1-2 canonical pages only per topic
- Include concrete drills and mnemonics tuned to Jeopardy! style
- Identify clue-level failure modes (e.g., "you missed the wordplay hint in...")"#;

#[derive(Debug, sqlx::FromRow)]
struct MissedQuestion {
    pub question: String,
    pub answer: String,
    pub category: String,
    pub classifier_category: Option<String>,
}

#[derive(Deserialize)]
pub struct GenerateBody {
    pub days: i32,
}

pub async fn generate(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<GenerateBody>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;

    if state.config.openai_api_key.is_empty() {
        return Err(AppError::BadRequest(
            "AI study recommendations are currently disabled (no OPENAI_API_KEY configured).".to_string(),
        ));
    }

    if body.days < 1 || body.days > 365 {
        return Err(AppError::BadRequest("days must be between 1 and 365".to_string()));
    }

    let time_period_end = Utc::now().naive_utc();
    let time_period_start = (Utc::now() - Duration::days(body.days as i64)).naive_utc();

    // Query incorrect attempts with question details
    let missed: Vec<MissedQuestion> = sqlx::query_as(
        "SELECT qa.id as attempt_id, qa.answered_at,
           jq.question, jq.answer, jq.category, jq.classifier_category
         FROM question_attempts qa
         JOIN jeopardy_questions jq ON qa.question_id = jq.id
         WHERE qa.user_id = $1 AND qa.correct = false
           AND qa.answered_at >= $2 AND qa.answered_at <= $3
         ORDER BY qa.answered_at DESC",
    )
    .bind(user_id)
    .bind(time_period_start)
    .bind(time_period_end)
    .fetch_all(&state.pool)
    .await?;

    let question_count = missed.len() as i32;

    // Group by classifier_category, max 10 questions per category
    let mut grouped: HashMap<String, Vec<&MissedQuestion>> = HashMap::new();
    for q in &missed {
        let key = q.classifier_category.clone().unwrap_or_else(|| "Uncategorized".to_string());
        let entry = grouped.entry(key).or_default();
        if entry.len() < 10 {
            entry.push(q);
        }
    }

    // Build text representation
    let mut questions_text = String::new();
    for (category, questions) in &grouped {
        questions_text.push_str(&format!("\n[{}]\n", category));
        for (i, q) in questions.iter().enumerate() {
            questions_text.push_str(&format!(
                "{}. Clue: \"{}\" Response: \"{}\" Original Category: {}\n",
                i + 1,
                q.answer,
                q.question,
                q.category
            ));
        }
    }

    let user_prompt = format!(
        r#"The user answered {} Jeopardy! clues incorrectly in the past {} day(s).

Here are the missed clues, grouped by category:
{}

Return your response as JSON in this exact format:
{{
  "analysis": "2-3 sentence pattern summary",
  "topics": [
    {{
      "topic": "Memorable name",
      "explanation": "Why this is a knowledge gap",
      "readings": ["Source 1", "Source 2"],
      "wikipedia": ["https://en.wikipedia.org/wiki/Page"],
      "strategies": ["Drill 1", "Drill 2"]
    }}
  ]
}}"#,
        question_count, body.days, questions_text
    );

    // Call OpenAI Chat Completions API
    let client = reqwest::Client::new();
    let response = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", state.config.openai_api_key))
        .json(&serde_json::json!({
            "model": "gpt-4o",
            "temperature": 0.7,
            "response_format": { "type": "json_object" },
            "messages": [
                { "role": "system", "content": SYSTEM_PROMPT },
                { "role": "user", "content": user_prompt }
            ]
        }))
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("OpenAI request failed: {}", e)))?;

    let openai_response: Value = response
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to parse OpenAI response: {}", e)))?;

    let content = openai_response["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| AppError::Internal("No content in OpenAI response".to_string()))?;

    let llm_response: Value = serde_json::from_str(content)
        .map_err(|e| AppError::Internal(format!("Failed to parse LLM JSON: {}", e)))?;

    let analysis = llm_response["analysis"]
        .as_str()
        .unwrap_or("")
        .to_string();

    let topics = llm_response["topics"].clone();

    // Save to DB
    let row: (i32, chrono::NaiveDateTime) = sqlx::query_as(
        "INSERT INTO study_recommendations (user_id, days_analyzed, analysis, recommendations, question_count, time_period_start, time_period_end)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id, generated_at",
    )
    .bind(user_id)
    .bind(body.days)
    .bind(&analysis)
    .bind(&topics)
    .bind(question_count)
    .bind(time_period_start)
    .bind(time_period_end)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(json!({
        "id": row.0,
        "user_id": user_id,
        "generated_at": row.1,
        "days_analyzed": body.days,
        "analysis": analysis,
        "recommendations": topics,
        "question_count": question_count,
        "time_period_start": time_period_start,
        "time_period_end": time_period_end,
    })))
}

pub async fn latest(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;

    let rec: Option<StudyRecommendation> = sqlx::query_as(
        "SELECT * FROM study_recommendations WHERE user_id = $1 ORDER BY generated_at DESC LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?;

    Ok(Json(json!(rec)))
}

pub async fn history(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;

    let recs: Vec<StudyRecommendation> = sqlx::query_as(
        "SELECT * FROM study_recommendations WHERE user_id = $1 ORDER BY generated_at DESC",
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(json!(recs)))
}
