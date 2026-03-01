use crate::clock::now_iso8601;
use crate::config::SchedulerConfig;
use crate::event::contracts::{named_trigger, scheduler_fired, scheduler_notice};
use crate::scheduler::{
    ScheduleAction, ScheduleStore, ScheduleUpsertInput, SCHEDULE_SCOPE_DEFAULT,
    SELF_IMPROVEMENT_SCHEDULE_ID,
};
use crate::{record_event, AppState};
use serde_json::json;
use std::sync::Arc;
use time::OffsetDateTime;

const SCHEDULER_LOOP_DUE_LIMIT: usize = 64;
const SCHEDULER_MIN_TICK_INTERVAL_MS: u64 = 100;

pub(crate) async fn start_scheduler(
    state: AppState,
    schedule_store: Arc<ScheduleStore>,
    config: SchedulerConfig,
) -> Result<(), String> {
    if !config.enabled {
        return Ok(());
    }

    let policy = config.self_improvement.clone().ok_or_else(|| {
        "scheduler.self_improvement is required when scheduler.enabled=true".to_string()
    })?;

    let action_event = match policy.action {
        ScheduleAction::EmitEvent { ref event, .. } => event.clone(),
        ScheduleAction::EmitMessage { .. } => {
            return Err("scheduler.self_improvement.action.kind must be 'emit_event'".to_string());
        }
    };
    if action_event != "self_improvement.run" {
        return Err(
            "scheduler.self_improvement.action.event must be 'self_improvement.run'".to_string(),
        );
    }

    let bootstrap_schedule = ScheduleUpsertInput {
        id: SELF_IMPROVEMENT_SCHEDULE_ID.to_string(),
        recurrence: policy.recurrence,
        timezone: policy.timezone,
        action: policy.action,
        enabled: policy.enabled,
    };
    schedule_store
        .upsert(SCHEDULE_SCOPE_DEFAULT, bootstrap_schedule)
        .await?;

    let tick_interval_ms = config.tick_interval_ms.max(SCHEDULER_MIN_TICK_INTERVAL_MS);

    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(std::time::Duration::from_millis(tick_interval_ms));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            interval.tick().await;

            let due = match schedule_store
                .acquire_due(OffsetDateTime::now_utc(), SCHEDULER_LOOP_DUE_LIMIT)
                .await
            {
                Ok(value) => value,
                Err(err) => {
                    println!("SCHEDULER_DUE_LOAD_ERROR error={}", err);
                    continue;
                }
            };

            for schedule in due {
                let scheduled_at = schedule.next_fire_at.clone();
                if let Err(err) = emit_scheduled_event(
                    &state,
                    &schedule.action,
                    schedule.id.as_str(),
                    scheduled_at.as_str(),
                )
                .await
                {
                    println!(
                        "SCHEDULER_EMIT_ERROR schedule_id={} scheduled_at={} error={}",
                        schedule.id, scheduled_at, err
                    );
                    continue;
                }

                let next_fire = match schedule.next_fire_after_current() {
                    Ok(value) => value,
                    Err(err) => {
                        println!(
                            "SCHEDULER_NEXT_FIRE_ERROR schedule_id={} scheduled_at={} error={}",
                            schedule.id, scheduled_at, err
                        );
                        continue;
                    }
                };

                if let Err(err) = schedule_store
                    .update_next_fire(
                        schedule.scope.as_str(),
                        schedule.id.as_str(),
                        next_fire.as_deref(),
                    )
                    .await
                {
                    println!(
                        "SCHEDULER_NEXT_FIRE_UPDATE_ERROR schedule_id={} error={}",
                        schedule.id, err
                    );
                }
            }
        }
    });

    Ok(())
}

async fn emit_scheduled_event(
    state: &AppState,
    action: &ScheduleAction,
    schedule_id: &str,
    scheduled_at: &str,
) -> Result<(), String> {
    let duplicate = state
        .db
        .exists_scheduler_fired(schedule_id, scheduled_at)
        .await
        .map_err(|err| err.to_string())?;

    if duplicate {
        println!(
            "SCHEDULER_DUPLICATE_SKIP schedule_id={} scheduled_at={}",
            schedule_id, scheduled_at
        );
        return Ok(());
    }

    if schedule_id == SELF_IMPROVEMENT_SCHEDULE_ID {
        return emit_self_improvement_event(state, action, schedule_id, scheduled_at).await;
    }
    emit_scheduler_notice(state, action, schedule_id, scheduled_at).await
}

async fn emit_self_improvement_event(
    state: &AppState,
    action: &ScheduleAction,
    schedule_id: &str,
    scheduled_at: &str,
) -> Result<(), String> {
    match action {
        ScheduleAction::EmitEvent { event, payload } => {
            let mut action_payload = payload.clone();
            if let Some(map) = action_payload.as_object_mut() {
                if !map.contains_key("target") {
                    map.insert("target".to_string(), json!("all"));
                }
                if !map.contains_key("reason") {
                    map.insert("reason".to_string(), json!("scheduled"));
                }
                map.insert("schedule_id".to_string(), json!(schedule_id));
                map.insert("scheduled_at".to_string(), json!(scheduled_at));
                map.insert("created_at".to_string(), json!(now_iso8601()));
                map.insert("created_by".to_string(), json!("scheduler"));
            }

            let action_event = named_trigger("scheduler", event, action_payload.clone());
            record_event(state, action_event).await;

            let fired_at = now_iso8601();
            let fired_event = scheduler_fired(
                json!({
                    "schedule_id": schedule_id,
                    "scheduled_at": scheduled_at,
                    "fired_at": fired_at,
                    "action": {
                        "event": event,
                        "payload": action_payload,
                    }
                }),
                event,
            );
            record_event(state, fired_event).await;
            Ok(())
        }
        ScheduleAction::EmitMessage { .. } => Err(
            "self-improvement policy must use emit_event action, emit_message is not allowed"
                .to_string(),
        ),
    }
}

async fn emit_scheduler_notice(
    state: &AppState,
    action: &ScheduleAction,
    schedule_id: &str,
    scheduled_at: &str,
) -> Result<(), String> {
    let action_json = serde_json::to_value(action).map_err(|err| err.to_string())?;
    let notice_event = scheduler_notice(json!({
        "schedule_id": schedule_id,
        "scheduled_at": scheduled_at,
        "noticed_at": now_iso8601(),
        "action": action_json,
    }));
    record_event(state, notice_event).await;

    let fired_event = scheduler_fired(
        json!({
            "schedule_id": schedule_id,
            "scheduled_at": scheduled_at,
            "fired_at": now_iso8601(),
            "action": action,
            "disposition": "notice_only",
        }),
        "scheduler.notice",
    );
    record_event(state, fired_event).await;
    Ok(())
}
