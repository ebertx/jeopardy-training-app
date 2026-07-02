//! Minimal OpenAI Chat Completions helper: JSON-mode call, returns the
//! assistant message content parsed as JSON.
use serde_json::Value;

use crate::error::AppError;

pub async fn chat_json(
    api_key: &str,
    model: &str,
    system: &str,
    user: &str,
    temperature: f64,
) -> Result<Value, AppError> {
    let client = reqwest::Client::new();
    let response = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&serde_json::json!({
            "model": model,
            "temperature": temperature,
            "response_format": { "type": "json_object" },
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user }
            ]
        }))
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("OpenAI request failed: {e}")))?;

    let body: Value = response
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to parse OpenAI response: {e}")))?;

    let content = body["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| AppError::Internal(format!("No content in OpenAI response: {body}")))?;

    serde_json::from_str(content)
        .map_err(|e| AppError::Internal(format!("LLM returned non-JSON content: {e}")))
}
