use crate::clock::now_iso8601;
use crate::db::Db;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use time::{
    format_description::well_known::Rfc3339, Date, Duration, Month, OffsetDateTime, Time,
    UtcOffset, Weekday,
};

pub const SCHEDULE_SCOPE_DEFAULT: &str = "global";
pub const SELF_IMPROVEMENT_SCHEDULE_ID: &str = "self_improvement.default";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleUpsertInput {
    pub id: String,
    pub recurrence: ScheduleRecurrence,
    pub timezone: String,
    pub action: ScheduleAction,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleRecord {
    pub id: String,
    pub recurrence: ScheduleRecurrence,
    pub timezone: String,
    pub action: ScheduleAction,
    pub enabled: bool,
    pub next_fire_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct DueSchedule {
    pub scope: String,
    pub id: String,
    pub recurrence: ScheduleRecurrence,
    pub timezone: String,
    pub action: ScheduleAction,
    pub next_fire_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScheduleRecurrence {
    Once { at: String },
    Daily { at: String },
    Weekly { weekdays: Vec<u8>, at: String },
    Monthly { day: u8, at: String },
    Interval { seconds: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScheduleAction {
    EmitEvent { event: String, payload: Value },
}

#[derive(Clone)]
pub struct ScheduleStore {
    db: Arc<Db>,
}

impl ScheduleStore {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    pub async fn upsert(
        &self,
        scope: &str,
        input: ScheduleUpsertInput,
    ) -> Result<ScheduleRecord, String> {
        validate_scope(scope)?;
        let id = validate_schedule_id(input.id.as_str())?;
        input.recurrence.validate()?;
        input.action.validate()?;
        let timezone_offset = parse_timezone_offset(input.timezone.as_str())?;
        let now = now_utc();
        let next_fire_at = if input.enabled {
            input
                .recurrence
                .next_fire_at(now, timezone_offset)?
                .map(format_rfc3339)
        } else {
            None
        };

        let recurrence_json = serde_json::to_string(&input.recurrence)
            .map_err(|err| format!("failed to encode recurrence: {}", err))?;
        let action_json = serde_json::to_string(&input.action)
            .map_err(|err| format!("failed to encode action: {}", err))?;

        let updated_at = self
            .db
            .upsert_schedule(
                scope,
                id.as_str(),
                recurrence_json.as_str(),
                input.timezone.as_str(),
                action_json.as_str(),
                input.enabled,
                next_fire_at.as_deref(),
            )
            .await
            .map_err(|err| err.to_string())?;

        Ok(ScheduleRecord {
            id,
            recurrence: input.recurrence,
            timezone: input.timezone,
            action: input.action,
            enabled: input.enabled,
            next_fire_at,
            updated_at,
        })
    }

    pub async fn list(&self, scope: &str) -> Result<Vec<ScheduleRecord>, String> {
        validate_scope(scope)?;
        let rows = self
            .db
            .load_schedules(scope)
            .await
            .map_err(|err| err.to_string())?;
        rows.into_iter().map(parse_schedule_row).collect()
    }

    pub async fn remove(&self, scope: &str, id: &str) -> Result<bool, String> {
        validate_scope(scope)?;
        let valid_id = validate_schedule_id(id)?;
        self.db
            .remove_schedule(scope, valid_id.as_str())
            .await
            .map_err(|err| err.to_string())
    }

    pub async fn acquire_due(
        &self,
        now: OffsetDateTime,
        limit: usize,
    ) -> Result<Vec<DueSchedule>, String> {
        let rows = self
            .db
            .acquire_due_schedules(format_rfc3339(now).as_str(), limit)
            .await
            .map_err(|err| err.to_string())?;

        rows.into_iter()
            .map(|row| {
                let (scope, id, recurrence_json, timezone, action_json, next_fire_at) = row;
                let recurrence: ScheduleRecurrence = serde_json::from_str(recurrence_json.as_str())
                    .map_err(|err| format!("failed to decode recurrence for '{}': {}", id, err))?;
                recurrence.validate()?;
                let action: ScheduleAction = serde_json::from_str(action_json.as_str())
                    .map_err(|err| format!("failed to decode action for '{}': {}", id, err))?;
                action.validate()?;
                parse_rfc3339(next_fire_at.as_str())?;
                parse_timezone_offset(timezone.as_str())?;
                Ok(DueSchedule {
                    scope,
                    id,
                    recurrence,
                    timezone,
                    action,
                    next_fire_at,
                })
            })
            .collect()
    }

    pub async fn update_next_fire(
        &self,
        scope: &str,
        id: &str,
        next_fire_at: Option<&str>,
    ) -> Result<(), String> {
        validate_scope(scope)?;
        let valid_id = validate_schedule_id(id)?;
        if let Some(value) = next_fire_at {
            parse_rfc3339(value)?;
        }
        self.db
            .update_schedule_next_fire(scope, valid_id.as_str(), next_fire_at)
            .await
            .map_err(|err| err.to_string())
    }
}

impl DueSchedule {
    pub fn next_fire_after_current(&self) -> Result<Option<String>, String> {
        let scheduled_at = parse_rfc3339(self.next_fire_at.as_str())?;
        let timezone_offset = parse_timezone_offset(self.timezone.as_str())?;
        self.recurrence
            .next_fire_after(scheduled_at, timezone_offset)
            .map(|value| value.map(format_rfc3339))
    }
}

impl ScheduleRecurrence {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Once { at } => {
                parse_rfc3339(at.as_str())?;
            }
            Self::Daily { at } => {
                parse_time_of_day(at.as_str())?;
            }
            Self::Weekly { weekdays, at } => {
                if weekdays.is_empty() {
                    return Err("weekly recurrence requires at least one weekday".to_string());
                }
                for weekday in weekdays {
                    if !(1..=7).contains(weekday) {
                        return Err(format!(
                            "weekly recurrence weekday must be 1..7, got {}",
                            weekday
                        ));
                    }
                }
                parse_time_of_day(at.as_str())?;
            }
            Self::Monthly { day, at } => {
                if !(1..=31).contains(day) {
                    return Err(format!("monthly recurrence day must be 1..31, got {}", day));
                }
                parse_time_of_day(at.as_str())?;
            }
            Self::Interval { seconds } => {
                if *seconds == 0 {
                    return Err("interval recurrence seconds must be > 0".to_string());
                }
            }
        }
        Ok(())
    }

    pub fn next_fire_at(
        &self,
        now: OffsetDateTime,
        timezone_offset: UtcOffset,
    ) -> Result<Option<OffsetDateTime>, String> {
        self.next_fire_after(now, timezone_offset)
    }

    pub fn next_fire_after(
        &self,
        base_utc: OffsetDateTime,
        timezone_offset: UtcOffset,
    ) -> Result<Option<OffsetDateTime>, String> {
        match self {
            Self::Once { at } => {
                let at_utc = parse_rfc3339(at.as_str())?;
                if at_utc <= base_utc {
                    Ok(None)
                } else {
                    Ok(Some(at_utc))
                }
            }
            Self::Daily { at } => next_daily(base_utc, timezone_offset, at.as_str()).map(Some),
            Self::Weekly { weekdays, at } => {
                next_weekly(base_utc, timezone_offset, weekdays, at.as_str()).map(Some)
            }
            Self::Monthly { day, at } => {
                next_monthly(base_utc, timezone_offset, *day, at.as_str()).map(Some)
            }
            Self::Interval { seconds } => {
                let delta = Duration::seconds(*seconds as i64);
                Ok(Some(base_utc + delta))
            }
        }
    }
}

impl ScheduleAction {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::EmitEvent { event, payload } => {
                let event_name = event.trim();
                if event_name.is_empty() {
                    return Err("action.event must not be empty".to_string());
                }
                if !payload.is_object() {
                    return Err("action.payload must be an object".to_string());
                }
            }
        }
        Ok(())
    }
}

fn now_utc() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

fn validate_scope(scope: &str) -> Result<(), String> {
    if scope.trim().is_empty() {
        return Err("scope must not be empty".to_string());
    }
    Ok(())
}

fn validate_schedule_id(id: &str) -> Result<String, String> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return Err("id must not be empty".to_string());
    }
    if trimmed.len() > 128 {
        return Err("id must be <= 128 chars".to_string());
    }
    Ok(trimmed.to_string())
}

fn parse_schedule_row(
    row: (
        String,
        String,
        String,
        String,
        String,
        i64,
        Option<String>,
        String,
    ),
) -> Result<ScheduleRecord, String> {
    let (_scope, id, recurrence_json, timezone, action_json, enabled, next_fire_at, updated_at) =
        row;
    let recurrence: ScheduleRecurrence = serde_json::from_str(recurrence_json.as_str())
        .map_err(|err| format!("failed to decode recurrence for '{}': {}", id, err))?;
    let action: ScheduleAction = serde_json::from_str(action_json.as_str())
        .map_err(|err| format!("failed to decode action for '{}': {}", id, err))?;
    recurrence.validate()?;
    action.validate()?;
    parse_timezone_offset(timezone.as_str())?;
    if let Some(value) = next_fire_at.as_ref() {
        parse_rfc3339(value.as_str())?;
    }

    Ok(ScheduleRecord {
        id,
        recurrence,
        timezone,
        action,
        enabled: enabled != 0,
        next_fire_at,
        updated_at,
    })
}

fn parse_rfc3339(value: &str) -> Result<OffsetDateTime, String> {
    OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|err| format!("invalid RFC3339 datetime '{}': {}", value, err))
}

fn format_rfc3339(value: OffsetDateTime) -> String {
    value.format(&Rfc3339).unwrap_or_else(|_| now_iso8601())
}

fn parse_time_of_day(value: &str) -> Result<Time, String> {
    let segments = value.split(':').collect::<Vec<_>>();
    if segments.len() != 2 && segments.len() != 3 {
        return Err(format!(
            "time '{}' must be in HH:MM or HH:MM:SS format",
            value
        ));
    }

    let hour = segments[0]
        .parse::<u8>()
        .map_err(|_| format!("invalid hour in '{}': {}", value, segments[0]))?;
    let minute = segments[1]
        .parse::<u8>()
        .map_err(|_| format!("invalid minute in '{}': {}", value, segments[1]))?;
    let second = if segments.len() == 3 {
        segments[2]
            .parse::<u8>()
            .map_err(|_| format!("invalid second in '{}': {}", value, segments[2]))?
    } else {
        0
    };

    Time::from_hms(hour, minute, second).map_err(|err| format!("invalid time '{}': {}", value, err))
}

pub fn parse_timezone_offset(value: &str) -> Result<UtcOffset, String> {
    let tz = value.trim();
    if tz.eq_ignore_ascii_case("utc") || tz.eq_ignore_ascii_case("etc/utc") || tz == "Z" {
        return Ok(UtcOffset::UTC);
    }
    if tz == "Asia/Tokyo" {
        return UtcOffset::from_hms(9, 0, 0)
            .map_err(|err| format!("invalid fixed timezone '{}': {}", tz, err));
    }

    if tz.len() == 6 && (tz.starts_with('+') || tz.starts_with('-')) && tz.as_bytes()[3] == b':' {
        let sign = if tz.starts_with('-') { -1 } else { 1 };
        let hour = tz[1..3]
            .parse::<i8>()
            .map_err(|_| format!("invalid timezone hour in '{}'", tz))?;
        let minute = tz[4..6]
            .parse::<i8>()
            .map_err(|_| format!("invalid timezone minute in '{}'", tz))?;
        let h = hour * sign;
        let m = minute * sign;
        return UtcOffset::from_hms(h, m, 0)
            .map_err(|err| format!("invalid timezone offset '{}': {}", tz, err));
    }

    Err(format!(
        "unsupported timezone '{}'; use UTC, Asia/Tokyo, or ±HH:MM",
        tz
    ))
}

fn next_daily(
    base_utc: OffsetDateTime,
    offset: UtcOffset,
    at: &str,
) -> Result<OffsetDateTime, String> {
    let local = base_utc.to_offset(offset);
    let at_time = parse_time_of_day(at)?;
    let mut date = local.date();
    let mut candidate = date
        .with_time(at_time)
        .assume_offset(offset)
        .to_offset(UtcOffset::UTC);
    if candidate <= base_utc {
        date = date
            .next_day()
            .ok_or_else(|| "failed to compute next day for daily recurrence".to_string())?;
        candidate = date
            .with_time(at_time)
            .assume_offset(offset)
            .to_offset(UtcOffset::UTC);
    }
    Ok(candidate)
}

fn next_weekly(
    base_utc: OffsetDateTime,
    offset: UtcOffset,
    weekdays: &[u8],
    at: &str,
) -> Result<OffsetDateTime, String> {
    let local = base_utc.to_offset(offset);
    let at_time = parse_time_of_day(at)?;
    let local_date = local.date();

    for delta in 0..8 {
        let date = if delta == 0 {
            local_date
        } else {
            local_date
                .checked_add(Duration::days(delta))
                .ok_or_else(|| "failed to compute weekly date".to_string())?
        };
        let weekday = weekday_index(date.weekday());
        if !weekdays.contains(&weekday) {
            continue;
        }

        let candidate = date
            .with_time(at_time)
            .assume_offset(offset)
            .to_offset(UtcOffset::UTC);
        if candidate > base_utc {
            return Ok(candidate);
        }
    }

    Err("failed to compute weekly next fire".to_string())
}

fn next_monthly(
    base_utc: OffsetDateTime,
    offset: UtcOffset,
    day: u8,
    at: &str,
) -> Result<OffsetDateTime, String> {
    let local = base_utc.to_offset(offset);
    let at_time = parse_time_of_day(at)?;

    let (mut year, mut month) = (local.year(), local.month());
    for _ in 0..24 {
        let max_day = days_in_month(year, month);
        let target_day = day.min(max_day);
        let date = Date::from_calendar_date(year, month, target_day)
            .map_err(|err| format!("failed to build monthly date: {}", err))?;
        let candidate = date
            .with_time(at_time)
            .assume_offset(offset)
            .to_offset(UtcOffset::UTC);
        if candidate > base_utc {
            return Ok(candidate);
        }
        let (next_year, next_month) = next_month(year, month);
        year = next_year;
        month = next_month;
    }

    Err("failed to compute monthly next fire".to_string())
}

fn weekday_index(weekday: Weekday) -> u8 {
    match weekday {
        Weekday::Monday => 1,
        Weekday::Tuesday => 2,
        Weekday::Wednesday => 3,
        Weekday::Thursday => 4,
        Weekday::Friday => 5,
        Weekday::Saturday => 6,
        Weekday::Sunday => 7,
    }
}

fn days_in_month(year: i32, month: Month) -> u8 {
    let first = Date::from_calendar_date(year, month, 1)
        .expect("valid first day of month should always be constructable");
    let (next_year, next_month) = next_month(year, month);
    let next_first = Date::from_calendar_date(next_year, next_month, 1)
        .expect("valid first day of next month should always be constructable");
    (next_first - first).whole_days() as u8
}

fn next_month(year: i32, month: Month) -> (i32, Month) {
    match month {
        Month::January => (year, Month::February),
        Month::February => (year, Month::March),
        Month::March => (year, Month::April),
        Month::April => (year, Month::May),
        Month::May => (year, Month::June),
        Month::June => (year, Month::July),
        Month::July => (year, Month::August),
        Month::August => (year, Month::September),
        Month::September => (year, Month::October),
        Month::October => (year, Month::November),
        Month::November => (year, Month::December),
        Month::December => (year + 1, Month::January),
    }
}
