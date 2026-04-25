use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::FromRow;

#[derive(Debug, FromRow, Serialize)]
pub struct StudyRecommendation {
    pub id: i32,
    pub user_id: i32,
    pub generated_at: NaiveDateTime,
    pub days_analyzed: i32,
    pub analysis: String,
    pub recommendations: serde_json::Value,
    pub question_count: i32,
    pub time_period_start: NaiveDateTime,
    pub time_period_end: NaiveDateTime,
}
