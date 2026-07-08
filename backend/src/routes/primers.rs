use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

/// Same model as the blindspot generator (see PACK_MODEL in blindspots.rs).
const PRIMER_MODEL: &str = "gpt-4o";
/// Low temperature: primers are factual reference content, not creative writing.
/// (Blindspots keep 0.7 because theme-finding benefits from it; confabulation doesn't.)
const PRIMER_TEMPERATURE: f64 = 0.2;

pub const CANON_TOPICS: &[&str] = &[
    "Opera",
    "Greek & Roman Mythology",
    "Norse Mythology",
    "Art Movements & Artists",
    "Baseball History",
    "New Deal & FDR",
    "Civil Rights Movement",
    "Shakespeare",
    "U.S. Presidents",
    "World Geography — Capitals & Rivers",
    "The Bible",
    "British Royals & History",
];

const PRIMER_SYSTEM_PROMPT: &str = "You write study primers for Jeopardy! preparation. \
Return JSON: {\"title\": string, \"content_md\": string}. content_md is a 1500-2500 word \
GitHub-flavored markdown study guide with these sections: \
## How this topic appears on Jeopardy (clue styles, frequency, typical difficulty); \
## The core canon (the facts that cover most clues, as markdown tables or tight lists — \
e.g. for opera: composer | work | plot one-liner | famous aria); \
## Clue angles & pivot words (the phrasings and giveaway words clues hinge on); \
## Common traps & confusions (the mix-ups clue writers exploit: commonly confused pairs, \
famous misattributions, items people wrongly include in this topic's canonical groups — \
state each trap and the correct fact); \
## Practice pairs (10 sample clue -> correct response pairs in Jeopardy style). \
Be specific and factual; prefer canonical, frequently-tested material over trivia depth. \
ACCURACY RULES: attribution facts (nationality, era, who-wrote/painted/composed-what) must be \
exact — they are the pivots Jeopardy clues hinge on, and an error here teaches the player a \
wrong answer. Never group people by nationality/era/movement unless every member truly belongs \
(e.g., Mozart is Austrian — never file him under Italian composers). Label things by what they \
are (an orchestral piece is not an aria). If unsure of a fact, omit it; a shorter correct \
primer beats a longer wrong one. Where a canonical confusion exists, call out the trap \
explicitly instead of repeating it. \
Do NOT invent mnemonics or acronyms. Never reproduce traditional rhymes or verses from \
memory — give the plain ordered list of facts instead. \
TOPIC MEMBERSHIP: every item in the core canon must genuinely belong to the topic's \
category — if a famous adjacent figure does not belong (e.g., Julius Caesar was never a \
Roman emperor), put that in Common traps, not in the canon table.";

/// Second-pass system prompt: adversarial fact-check of the generated draft.
/// This is the layer that actually catches confabulated groupings and garbled
/// sequences; the generation-prompt rules only shrink what it has to catch.
const PRIMER_VERIFY_PROMPT: &str = "You are a ruthless fact-checker for Jeopardy! study \
material. The user message is a draft study primer in markdown. Verify EVERY factual claim: \
attributions (who wrote/composed/painted/did what), nationalities, eras, dates, counts, set \
memberships (exactly which items belong to a named group), ordered sequences (succession \
lists, chronological orders), quoted verses, and every practice-pair answer. Also verify \
TOPIC MEMBERSHIP: every item listed in the core canon must truly belong to the topic's \
category (e.g., Julius Caesar in a Roman Emperors table is wrong — he was never emperor; \
move such items to the traps section with the correct fact). Delete any invented mnemonic \
or acronym whose letters do not decode to the facts it claims to encode. Correct \
everything wrong. Delete any claim you cannot verify rather than keeping it. Keep the \
structure, tone, headings, and approximate length otherwise unchanged. \
Return JSON: {\"content_md\": string} — the corrected primer.";

pub fn slugify(topic: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = true; // suppress leading dash
    for c in topic.trim().to_lowercase().chars() {
        if c.is_alphanumeric() {
            out.push(c);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

fn primer_json(row: (i32, String, String, String, String, chrono::DateTime<chrono::Utc>), cached: bool) -> Value {
    let (id, slug, topic, source, content_md, created_at) = row;
    json!({ "id": id, "slug": slug, "topic": topic, "source": source,
            "contentMd": content_md, "createdAt": created_at, "cached": cached })
}

type PrimerRow = (i32, String, String, String, String, chrono::DateTime<chrono::Utc>);
const PRIMER_COLS: &str = "id, slug, topic, source, content_md, created_at";

pub async fn list(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let rows: Vec<(i32, String, String, String, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT id, slug, topic, source, created_at FROM primers ORDER BY created_at DESC",
    )
    .fetch_all(&state.pool)
    .await?;
    let primers: Vec<Value> = rows.into_iter()
        .map(|(id, slug, topic, source, at)| json!({ "id": id, "slug": slug, "topic": topic, "source": source, "createdAt": at }))
        .collect();
    Ok(Json(json!({
        "primers": primers,
        "canon": CANON_TOPICS,
        "configured": !state.config.openai_api_key.is_empty(),
    })))
}

pub async fn get_primer(
    State(state): State<Arc<AppState>>,
    _auth: AuthUser,
    Path(slug): Path<String>,
) -> Result<Json<Value>, AppError> {
    let sql = format!("SELECT {PRIMER_COLS} FROM primers WHERE slug = $1");
    let row: Option<PrimerRow> = sqlx::query_as(&sql).bind(&slug).fetch_optional(&state.pool).await?;
    row.map(|r| Json(primer_json(r, true)))
        .ok_or_else(|| AppError::NotFound("Primer not found".into()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateBody {
    pub topic: String,
    pub source: Option<String>,
}

pub async fn generate(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(body): Json<GenerateBody>,
) -> Result<Json<Value>, AppError> {
    let topic = body.topic.trim().to_string();
    if topic.is_empty() || topic.len() > 120 {
        return Err(AppError::BadRequest("Topic must be 1-120 characters".into()));
    }
    let source = match body.source.as_deref() {
        Some("canon") | None if CANON_TOPICS.contains(&topic.as_str()) => "canon",
        Some("blindspot") => "blindspot",
        _ => "custom",
    };
    let slug = slugify(&topic);
    if slug.is_empty() {
        return Err(AppError::BadRequest("Topic has no usable characters".into()));
    }

    let sel = format!("SELECT {PRIMER_COLS} FROM primers WHERE slug = $1");
    if let Some(row) = sqlx::query_as::<_, PrimerRow>(&sel).bind(&slug).fetch_optional(&state.pool).await? {
        return Ok(Json(primer_json(row, true)));
    }
    if state.config.openai_api_key.is_empty() {
        return Err(AppError::BadRequest("Primer generation is not configured (no API key)".into()));
    }

    let user_prompt = format!("Topic: {topic}\nReturn the JSON now.");
    let v = crate::openai::chat_json(
        &state.config.openai_api_key,
        PRIMER_MODEL,
        PRIMER_SYSTEM_PROMPT,
        &user_prompt,
        PRIMER_TEMPERATURE,
    )
    .await?;
    let content_md = v["content_md"]
        .as_str()
        .ok_or_else(|| AppError::Internal("LLM response missing content_md".into()))?
        .to_string();
    if content_md.len() < 500 {
        return Err(AppError::Internal("LLM primer implausibly short".into()));
    }

    // Verification pass: a second call whose only job is fact-checking the draft.
    // Best-effort — a flaky verifier shouldn't lose the draft, so fall back to it
    // (with a warning) rather than failing the request.
    let content_md = match crate::openai::chat_json(
        &state.config.openai_api_key,
        PRIMER_MODEL,
        PRIMER_VERIFY_PROMPT,
        &content_md,
        0.0,
    )
    .await
    {
        Ok(vv) => match vv["content_md"].as_str() {
            Some(s) if s.len() >= 500 => s.to_string(),
            _ => {
                tracing::warn!("primer verify pass returned invalid content for '{topic}'; storing unverified draft");
                content_md
            }
        },
        Err(e) => {
            tracing::warn!("primer verify pass failed for '{topic}' ({e:?}); storing unverified draft");
            content_md
        }
    };

    // Concurrent-generation guard: first writer wins, everyone re-selects.
    sqlx::query(
        "INSERT INTO primers (slug, topic, content_md, model, source, requested_by)
         VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (slug) DO NOTHING",
    )
    .bind(&slug).bind(&topic).bind(&content_md)
    .bind(PRIMER_MODEL).bind(source).bind(auth.user_id)
    .execute(&state.pool)
    .await?;
    let row: PrimerRow = sqlx::query_as(&sel).bind(&slug).fetch_one(&state.pool).await?;
    Ok(Json(primer_json(row, false)))
}

#[cfg(test)]
mod tests {
    use super::slugify;

    #[test]
    fn slugify_basics() {
        assert_eq!(slugify("Greek & Roman Mythology"), "greek-roman-mythology");
        assert_eq!(slugify("  New Deal & FDR  "), "new-deal-fdr");
        assert_eq!(slugify("Opera"), "opera");
        assert_eq!(slugify("U.S. Presidents"), "u-s-presidents");
    }
}
