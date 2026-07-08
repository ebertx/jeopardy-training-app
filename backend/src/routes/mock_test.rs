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
