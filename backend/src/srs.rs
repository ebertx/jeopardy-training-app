//! Self-contained spaced-repetition scheduler (SM-2 derived, 3 ratings).
//!
//! Pure and deterministic: no DB, no wall clock, no randomness. The DB layer
//! reads `Outcome.interval_secs` to set `due = now() + interval_secs`.
//! Internals are intentionally hidden behind `schedule()` so they can later be
//! swapped for the `fsrs` crate without changing callers.

const LEARNING_STEPS_SECS: [i64; 2] = [60, 600];   // 1 min, 10 min
const RELEARNING_STEPS_SECS: [i64; 1] = [600];     // 10 min
const DAY_SECS: i64 = 86_400;

const STARTING_EASE: f64 = 2.5;
const MIN_EASE: f64 = 1.3;
const EASE_PENALTY: f64 = 0.20; // Wrong on a review card
const EASE_EASY_BONUS: f64 = 0.15; // TooEasy on a review card
const EASY_MULTIPLIER: f64 = 1.3;
const LAPSE_INTERVAL_MULT: f64 = 0.5; // shrink interval on lapse
const GRADUATING_INTERVAL_DAYS: f64 = 1.0;
const EASY_GRADUATING_INTERVAL_DAYS: f64 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rating {
    Wrong,
    GotIt,
    TooEasy,
}

impl Rating {
    pub fn from_wire(s: &str) -> Option<Rating> {
        match s {
            "wrong" => Some(Rating::Wrong),
            "got_it" => Some(Rating::GotIt),
            "too_easy" => Some(Rating::TooEasy),
            _ => None,
        }
    }
    /// Maps to the `correct` boolean recorded in question_attempts for stats.
    pub fn is_correct(self) -> bool {
        !matches!(self, Rating::Wrong)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardKind {
    Learning,
    Review,
    Relearning,
}

impl CardKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CardKind::Learning => "learning",
            CardKind::Review => "review",
            CardKind::Relearning => "relearning",
        }
    }
    pub fn from_str(s: &str) -> CardKind {
        match s {
            "review" => CardKind::Review,
            "relearning" => CardKind::Relearning,
            _ => CardKind::Learning,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Prev {
    pub state: CardKind,
    pub interval_days: f64,
    pub ease: f64,
    pub reps: i32,
    pub lapses: i32,
    pub step_index: i16,
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub state: CardKind,
    pub interval_days: f64,
    pub ease: f64,
    pub reps: i32,
    pub lapses: i32,
    pub step_index: i16,
    pub interval_secs: i64,
    pub requeue_in_session: bool,
}

pub fn schedule(prev: Option<Prev>, rating: Rating) -> Outcome {
    let prev = prev.unwrap_or(Prev {
        state: CardKind::Learning,
        interval_days: 0.0,
        ease: STARTING_EASE,
        reps: 0,
        lapses: 0,
        step_index: 0,
    });

    match prev.state {
        CardKind::Learning | CardKind::Relearning => step_through_learning(&prev, rating),
        CardKind::Review => grade_review(&prev, rating),
    }
}

fn steps_for(kind: CardKind) -> &'static [i64] {
    match kind {
        CardKind::Relearning => &RELEARNING_STEPS_SECS,
        _ => &LEARNING_STEPS_SECS,
    }
}

fn step_through_learning(prev: &Prev, rating: Rating) -> Outcome {
    let steps = steps_for(prev.state);
    match rating {
        Rating::Wrong => Outcome {
            state: prev.state,
            interval_days: prev.interval_days,
            ease: prev.ease,
            reps: prev.reps,
            lapses: prev.lapses,
            step_index: 0,
            interval_secs: steps[0],
            requeue_in_session: true,
        },
        Rating::GotIt => {
            let next_step = prev.step_index as usize + 1;
            if next_step >= steps.len() {
                // Graduate. Relearning resumes at its (already shrunk) interval;
                // fresh learning graduates to the standard 1-day interval.
                let interval_days = if prev.state == CardKind::Relearning {
                    prev.interval_days.max(GRADUATING_INTERVAL_DAYS).round()
                } else {
                    GRADUATING_INTERVAL_DAYS
                };
                Outcome {
                    state: CardKind::Review,
                    interval_days,
                    ease: prev.ease,
                    reps: prev.reps + 1,
                    lapses: prev.lapses,
                    step_index: 0,
                    interval_secs: (interval_days as i64) * DAY_SECS,
                    requeue_in_session: false,
                }
            } else {
                Outcome {
                    state: prev.state,
                    interval_days: prev.interval_days,
                    ease: prev.ease,
                    reps: prev.reps,
                    lapses: prev.lapses,
                    step_index: next_step as i16,
                    interval_secs: steps[next_step],
                    requeue_in_session: true,
                }
            }
        }
        Rating::TooEasy => {
            // A fresh learning card graduates to the flat easy interval. A
            // relearning card must not collapse below what a "Got it" would
            // give it (prev.interval_days), so keep a strict edge over Good and
            // floor at the easy graduating interval.
            let interval_days = if prev.state == CardKind::Relearning {
                (prev.interval_days.max(1.0) * EASY_MULTIPLIER)
                    .max(EASY_GRADUATING_INTERVAL_DAYS)
                    .round()
            } else {
                EASY_GRADUATING_INTERVAL_DAYS
            };
            Outcome {
                state: CardKind::Review,
                interval_days,
                ease: prev.ease,
                reps: prev.reps + 1,
                lapses: prev.lapses,
                step_index: 0,
                interval_secs: (interval_days as i64) * DAY_SECS,
                requeue_in_session: false,
            }
        }
    }
}

fn grade_review(prev: &Prev, rating: Rating) -> Outcome {
    match rating {
        Rating::Wrong => {
            let ease = (prev.ease - EASE_PENALTY).max(MIN_EASE);
            let interval_days = (prev.interval_days * LAPSE_INTERVAL_MULT).max(1.0).round();
            Outcome {
                state: CardKind::Relearning,
                interval_days,
                ease,
                reps: prev.reps,
                lapses: prev.lapses + 1,
                step_index: 0,
                interval_secs: RELEARNING_STEPS_SECS[0],
                requeue_in_session: true,
            }
        }
        Rating::GotIt => {
            let interval_days = (prev.interval_days * prev.ease).max(1.0).round();
            Outcome {
                state: CardKind::Review,
                interval_days,
                ease: prev.ease,
                reps: prev.reps + 1,
                lapses: prev.lapses,
                step_index: 0,
                interval_secs: (interval_days as i64) * DAY_SECS,
                requeue_in_session: false,
            }
        }
        Rating::TooEasy => {
            let ease = prev.ease + EASE_EASY_BONUS;
            let interval_days = (prev.interval_days * ease * EASY_MULTIPLIER).max(1.0).round();
            Outcome {
                state: CardKind::Review,
                interval_days,
                ease,
                reps: prev.reps + 1,
                lapses: prev.lapses,
                step_index: 0,
                interval_secs: (interval_days as i64) * DAY_SECS,
                requeue_in_session: false,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;

    #[test]
    fn new_card_wrong_stays_learning_step0_requeues() {
        let o = schedule(None, Rating::Wrong);
        assert!(matches!(o.state, CardKind::Learning));
        assert_eq!(o.step_index, 0);
        assert_eq!(o.interval_secs, 60);
        assert!(o.requeue_in_session);
        assert_eq!(o.reps, 0);
    }

    #[test]
    fn new_card_gotit_advances_to_second_learning_step() {
        let o = schedule(None, Rating::GotIt);
        assert!(matches!(o.state, CardKind::Learning));
        assert_eq!(o.step_index, 1);
        assert_eq!(o.interval_secs, 600);
        assert!(o.requeue_in_session);
    }

    #[test]
    fn new_card_tooeasy_graduates_to_review_four_days() {
        let o = schedule(None, Rating::TooEasy);
        assert!(matches!(o.state, CardKind::Review));
        assert_eq!(o.interval_days, 4.0);
        assert_eq!(o.interval_secs, 4 * DAY);
        assert!(!o.requeue_in_session);
        assert_eq!(o.reps, 1);
    }

    #[test]
    fn gotit_on_last_learning_step_graduates_one_day() {
        let prev = Prev { state: CardKind::Learning, interval_days: 0.0, ease: 2.5, reps: 0, lapses: 0, step_index: 1 };
        let o = schedule(Some(prev), Rating::GotIt);
        assert!(matches!(o.state, CardKind::Review));
        assert_eq!(o.interval_days, 1.0);
        assert_eq!(o.interval_secs, DAY);
        assert!(!o.requeue_in_session);
        assert_eq!(o.reps, 1);
    }

    #[test]
    fn review_gotit_multiplies_interval_by_ease() {
        let prev = Prev { state: CardKind::Review, interval_days: 10.0, ease: 2.5, reps: 3, lapses: 0, step_index: 0 };
        let o = schedule(Some(prev), Rating::GotIt);
        assert!(matches!(o.state, CardKind::Review));
        assert_eq!(o.interval_days, 25.0); // 10 * 2.5
        assert_eq!(o.ease, 2.5);
        assert_eq!(o.reps, 4);
        assert!(!o.requeue_in_session);
    }

    #[test]
    fn review_tooeasy_beats_gotit_and_raises_ease() {
        let prev = Prev { state: CardKind::Review, interval_days: 10.0, ease: 2.5, reps: 3, lapses: 0, step_index: 0 };
        let good = schedule(Some(prev.clone()), Rating::GotIt);
        let easy = schedule(Some(prev), Rating::TooEasy);
        assert!(easy.interval_days > good.interval_days);
        assert!(easy.ease > 2.5);
    }

    #[test]
    fn review_wrong_lapses_into_relearning_and_lowers_ease() {
        let prev = Prev { state: CardKind::Review, interval_days: 20.0, ease: 2.5, reps: 5, lapses: 1, step_index: 0 };
        let o = schedule(Some(prev), Rating::Wrong);
        assert!(matches!(o.state, CardKind::Relearning));
        assert_eq!(o.lapses, 2);
        assert_eq!(o.step_index, 0);
        assert_eq!(o.interval_secs, 600); // relearning step
        assert!(o.requeue_in_session);
        assert!((o.ease - 2.3).abs() < 1e-9); // 2.5 - 0.20
        assert_eq!(o.interval_days, 10.0); // shrunk: 20 * 0.5
    }

    #[test]
    fn relearning_gotit_graduates_back_to_review_at_shrunk_interval() {
        let prev = Prev { state: CardKind::Relearning, interval_days: 10.0, ease: 2.3, reps: 5, lapses: 2, step_index: 0 };
        let o = schedule(Some(prev), Rating::GotIt);
        assert!(matches!(o.state, CardKind::Review));
        assert_eq!(o.interval_days, 10.0);
        assert_eq!(o.interval_secs, 10 * DAY);
        assert!(!o.requeue_in_session);
    }

    #[test]
    fn ease_never_drops_below_floor() {
        let prev = Prev { state: CardKind::Review, interval_days: 5.0, ease: 1.35, reps: 4, lapses: 3, step_index: 0 };
        let o = schedule(Some(prev), Rating::Wrong);
        assert!(o.ease >= 1.3);
    }

    #[test]
    fn interval_rounds_to_whole_days_min_one() {
        let prev = Prev { state: CardKind::Review, interval_days: 1.0, ease: 1.3, reps: 1, lapses: 0, step_index: 0 };
        let o = schedule(Some(prev), Rating::GotIt);
        assert!(o.interval_days >= 1.0);
        assert_eq!(o.interval_days, o.interval_days.round());
    }

    #[test]
    fn rating_from_wire_parses_known_values_only() {
        assert!(matches!(Rating::from_wire("wrong"), Some(Rating::Wrong)));
        assert!(matches!(Rating::from_wire("got_it"), Some(Rating::GotIt)));
        assert!(matches!(Rating::from_wire("too_easy"), Some(Rating::TooEasy)));
        assert!(Rating::from_wire("hard").is_none());
    }

    #[test]
    fn mid_learning_wrong_resets_to_first_step() {
        // A new card advanced to learning step 1, then answered Wrong, must drop
        // back to step 0 and requeue, without graduating or counting a lapse.
        let prev = Prev { state: CardKind::Learning, interval_days: 0.0, ease: 2.5, reps: 0, lapses: 0, step_index: 1 };
        let o = schedule(Some(prev), Rating::Wrong);
        assert!(matches!(o.state, CardKind::Learning));
        assert_eq!(o.step_index, 0);
        assert_eq!(o.interval_secs, 60);
        assert!(o.requeue_in_session);
        assert_eq!(o.lapses, 0);
    }

    #[test]
    fn relearning_tooeasy_is_at_least_relearning_gotit() {
        // Easy must never yield a shorter interval than Good on a relearning card.
        let prev = Prev { state: CardKind::Relearning, interval_days: 10.0, ease: 2.3, reps: 5, lapses: 2, step_index: 0 };
        let good = schedule(Some(prev.clone()), Rating::GotIt);
        let easy = schedule(Some(prev), Rating::TooEasy);
        assert!(matches!(easy.state, CardKind::Review));
        assert!(easy.interval_days >= good.interval_days);
        // And strictly beats it here (10 * 1.3 = 13 > 10).
        assert!(easy.interval_days > good.interval_days);
        assert_eq!(easy.interval_secs, (easy.interval_days as i64) * 86_400);
    }

    #[test]
    fn new_card_tooeasy_still_flat_four_days() {
        // The relearning fix must not change the fresh-learning Easy graduation.
        let o = schedule(None, Rating::TooEasy);
        assert!(matches!(o.state, CardKind::Review));
        assert_eq!(o.interval_days, 4.0);
    }
}
