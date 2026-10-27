# Spec Delta

## ADDED Requirements

### Requirement: Reminder setup command

`aidhd reminders` SHALL configure recurring proactive prompts without the user editing cron by hand. It SHALL support a preview that writes nothing, an enable action that installs a default morning and evening cadence, and a disable action that stops future prompts. It SHALL reuse the existing notification budget, quiet hours, and dedup, and SHALL NOT introduce a second scheduler or bypass those bounds.

#### Scenario: Enable the default cadence

- **WHEN** the user runs `reminders enable`
- **THEN** a recurring prompt is configured and a summary of the schedule is printed

#### Scenario: Preview writes nothing

- **WHEN** the user runs `reminders --dry-run`
- **THEN** it prints the schedule that would be created and makes no change

#### Scenario: Disable stops prompts

- **WHEN** the user runs `reminders disable`
- **THEN** no further proactive prompts are sent

#### Scenario: Quiet hours and budget are respected

- **WHEN** a reminder is due inside quiet hours or beyond the notification budget
- **THEN** no prompt is sent

#### Scenario: No second scheduler

- **WHEN** reminders are enabled
- **THEN** each prompt uses the same next-action selection as `aidhd next` and `aidhd due --notify`
