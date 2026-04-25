use chrono::NaiveDate;
use serde::Serialize;
use sqlx::FromRow;

#[derive(Debug, FromRow, Serialize)]
pub struct Question {
    pub id: i32,
    pub question: Option<String>,
    pub answer: Option<String>,
    pub category: Option<String>,
    pub classifier_category: Option<String>,
    pub clue_value: Option<i32>,
    pub round: Option<i32>,
    pub air_date: Option<NaiveDate>,
    pub notes: Option<String>,
}
