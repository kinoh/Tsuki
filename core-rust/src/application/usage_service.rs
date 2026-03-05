use crate::clock::now_iso8601;
use crate::db::UsageStatRecord;
use crate::llm::LlmResponse;
use crate::AppState;

pub(crate) async fn record_llm_usage(
    state: &AppState,
    user_id: &str,
    agent_name: &str,
    response: &LlmResponse,
) {
    let Some(usage) = response.usage.as_ref() else {
        return;
    };
    if response.response_id.trim().is_empty() {
        return;
    }

    let record = UsageStatRecord {
        id: response.response_id.clone(),
        user_id: user_id.to_string(),
        agent_name: agent_name.to_string(),
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        total_tokens: usage.total_tokens,
        reasoning_tokens: usage.reasoning_tokens,
        cached_input_tokens: usage.cached_input_tokens,
        created_at: now_iso8601(),
    };

    if let Err(err) = state.db.insert_usage_stat(record).await {
        eprintln!(
            "USAGE_RECORD_ERROR user_id={} agent_name={} response_id={} error={}",
            user_id, agent_name, response.response_id, err
        );
    }
}
