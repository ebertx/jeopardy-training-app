use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::FromRow;

#[derive(Debug, FromRow, Serialize)]
pub struct QuestionMastery {
    pub id: i32,
    pub user_id: i32,
    pub question_id: i32,
    pub consecutive_correct: i32,
    pub mastered: bool,
    pub mastered_at: Option<NaiveDateTime>,
    pub last_attempt_at: NaiveDateTime,
}
