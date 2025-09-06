# Scheduler for LLM agents

## Configuration

### Environment Variables

- **TZ** (Required): Timezone specification for schedule management
  - Example: `export TZ="Asia/Tokyo"`
  - Used for interpreting schedule times and calculating when schedules should fire
  - Must be a valid timezone identifier (IANA Time Zone Database)

- **SCHEDULER_LOOP_INTERVAL_MS** (Optional): Scheduler event loop interval in milliseconds
  - Example: `export SCHEDULER_LOOP_INTERVAL_MS=1000`
  - Controls how frequently the scheduler checks and fires due notifications
  - Default: `60000` (60 seconds)
  - For testing, set to a lower value (e.g., `100` or `500`) to make schedule firing and notification delivery faster

## Features

- Schedule message notifications at specific times
- Support for one-time and daily recurring schedules
- Real-time notification delivery via MCP resources
- Timezone-aware schedule management
- Simple interface designed for LLM agent integration

## Usage Patterns

### One-time Notifications

Schedule a single notification at a specific time:

```json
{
  "name": "meeting_reminder",
  "time": "2024-01-15T14:30:00",
  "cycle": "none",
  "message": "Team meeting in 30 minutes"
}
```

### Daily Recurring Schedules

Create schedules that repeat every day at the same time:

```json
{
  "name": "daily_standup",
  "time": "09:00",
  "cycle": "daily", 
  "message": "Daily standup meeting in 15 minutes"
}
```

### Notification Consumption

Subscribe to the `fired_schedule` resource to receive real-time notifications when scheduled times arrive.

## Tools

### set_schedule

Creates or updates a scheduled notification.

#### Arguments

- **name** (Required): Unique identifier for the schedule
- **time** (Required): Time specification for when the schedule should fire
  - For one-time: ISO 8601 datetime string (e.g., "2024-01-15T14:30:00")
  - For daily: Time string in HH:MM format (e.g., "09:30")
- **cycle** (Required): Schedule repetition type
  - `"none"`: One-time schedule
  - `"daily"`: Repeats every day at the specified time
- **message** (Required): Notification message to deliver when the schedule fires

#### Response

Returns `Succeeded` when the schedule is successfully created or updated.

#### Errors

- Returns `Error: time: invalid format` if time format is invalid
- Returns `Error: cycle: invalid value` if cycle value is not "none" or "daily"
- Returns `Error: name: required` if name argument is missing
- Returns `Error: message: required` if message argument is missing

### get_schedules

Retrieves all currently active schedules.

#### Arguments

None.

#### Response

Returns a list of all scheduled notifications with their details:

```json
[
  {
    "name": "meeting_reminder",
    "time": "2024-01-15T14:30:00",
    "cycle": "none",
    "message": "Team meeting in 30 minutes"
  },
  {
    "name": "daily_standup", 
    "time": "09:00",
    "cycle": "daily",
    "message": "Daily standup meeting in 15 minutes"
  }
]
```

#### Errors

None.

### remove_schedule

Removes a scheduled notification.

#### Arguments

- **name** (Required): Unique identifier of the schedule to remove

#### Response

Returns `Succeeded` when the schedule is successfully removed.

#### Errors

- Returns `Error: name: not found` if the specified schedule does not exist
- Returns `Error: name: required` if name argument is missing

## Resources

### fired_schedule://recent

This server provides only one resource: `fired_schedule://recent`.
This resource is intended to be used with the MCP protocol's subscribe feature. Clients can subscribe to this resource to receive real-time notifications (in JSON format) when a schedule fires.

#### Content

The resource response's `text` field contains fired schedule data in JSON format:

```json
[
  {
    "name": "meeting_reminder",
    "scheduled_time": "2024-01-15T14:30:00",
    "fired_time": "2024-01-15T14:30:01",
    "message": "Team meeting in 30 minutes"
  }
]
```

#### Notification

Resource update notifications follow MCP's standard format with uri and title parameters identifying the changed resource.

This mechanism allows LLM agents and other clients to react to schedule events immediately, without polling.

#### Notes

- No resources other than `fired_schedule://recent` are provided.
- MCP clients should use the `subscribe` request to subscribe to this resource.

#### Errors

None.
