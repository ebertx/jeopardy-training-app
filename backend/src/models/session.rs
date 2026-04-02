use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::FromRow;

#[derive(Debug, FromRow, Serialize)]
pub struct QuizSession {
    pub id: i32,
    pub user_id: i32,
    pub started_at: NaiveDateTime,
    pub completed_at: Option<NaiveDateTime>,
    pub category_filter: Option<String>,
    pub is_review_session: bool,
}

#[derive(Debug, FromRow, Serialize)]
pub struct QuestionAttempt {
    pub id: i32,
    pub session_id: i32,
    pub question_id: i32,
    pub user_id: i32,
    pub correct: bool,
    pub answered_at: NaiveDateTime,
}
