use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, FromRow, Serialize)]
pub struct CoryatGame {
    pub id: i32,
    pub user_id: i32,
    pub started_at: NaiveDateTime,
    pub completed_at: Option<NaiveDateTime>,
    pub game_board: serde_json::Value,
    pub jeopardy_score: i32,
    pub double_j_score: i32,
    pub final_score: Option<i32>,
    pub current_round: i32,
    pub questions_answered: i32,
}

#[derive(Debug, Deserialize)]
pub struct CoryatAnswerRequest {
    pub round: String,
    pub col: i32,
    pub row: i32,
    pub result: String,
}
