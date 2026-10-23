# AIDHD Reference

> **Status warning:** AIDHD is an unfinished personal project. It has working prototypes and passing tests, but its architecture, UI, edge-case handling, and planned features are not production-polished. Use it as a behavioral and data-shape reference, not as a blueprint to copy uncritically.

## What it is

- BeeWare/Toga Python GUI app, built with Briefcase, targets Linux desktop and Android.
- Private GitHub repo: `AutoMas0n/AIDHD`.
- Description: "[WIP] Keep track of things, easier".
- Last push: 2025-09-26.

## Core behavior

- Task CRUD, search/filter, priorities 1-3, categories, tags, recurring tasks, subtasks.
- Local SQLite database at `~/.aidhd/tasks.db`.
- AI natural-language task parsing, smart suggestions, conversational responses.
- Multi-provider LLM: OpenAI, Anthropic, Ollama, local. Fallback to basic parsing when AI fails.
- Voice STT/TTS: desktop via SpeechRecognition/PyAudio/pyttsx3/playsound; Android via native jnius service.
- Notifications/reminders: libnotify on Linux, native Android notifications and alarms.
- Export/import: JSON, CSV, full DB backup/restore.
- Voice command phrases: "add task", "create task", "new task", "list tasks", "show tasks", "my tasks", "complete task", "finish task", "done with", "help".

## Database schema (durable asset)

Tables:

- `tasks`: `id` TEXT PK (UUID), `title` TEXT NOT NULL, `description`, `priority` INTEGER default 2 (1=High, 2=Medium, 3=Low), `status` default 'pending' (pending, in_progress, completed, cancelled), `category` default 'general', `due_date` ISO text, `reminder_date` ISO text, `created_at`, `updated_at`, `completed_at`, `is_recurring`, `recurrence_pattern` JSON, `parent_task_id`, `user_notes`, `ai_suggestions` JSON, `metadata` JSON.
- `task_tags`: task_id, tag.
- `voice_interactions`: id, timestamp, user_input, transcribed_text, ai_response, action_taken JSON, related_task_id.
- `settings`: key/value.

## AI layer

- `llm_client.py` has three prompt-driven operations:
  - `process_natural_language_task(user_input, context)` -> structured task info.
  - `suggest_task_improvements(task)` -> priority/category suggestions.
  - `generate_conversational_response(user_input, context)` -> natural language reply.
- Providers: OpenAI, Anthropic, Ollama/local. Config includes provider, api_key, base_url, model, temperature, max_tokens.
- AIDHD reads keys from config/env; octx contract requires credentials by host via `octx-creds`, never env.

## What not to port

- Toga views, dialogs, tap-to-speak button, GUI event loop.
- Android alarm receivers and in-app notification plumbing.
- Background "secretary" module (planned, not built): auto-reschedule, conflict resolution, reminders.
- Long-running scheduler behavior: octx arms run once and exit.

## Open questions for the port

1. Replace AIDHD or bridge to its DB? Android sandbox makes shared SQLite fragile; replacing with an arm-owned store is cleaner.
2. Language: Rust arm vs Python arm. Rust fits octx conventions; Python reuses code but bends the standalone-binary model.
3. Interaction model: verbs-first with AI on add/ask/suggest vs utterance-first.
4. v1 scope: CRUD + AI parse + export/import; voice/notifications optional; secretary out of scope.
5. Credentials/config: move AI keys to `octx creds add <host>` and config to octx config dir.

## Evidence sources

- GitHub repo `AutoMas0n/AIDHD`: README, requirements.md, current_state.md, todo.md, src/aidhd/*.py, tests/*.py.
- octx `ARCHITECTURE.md`, `SKILL-CONTRACT.md`, `ARMS-IDEA.md`.
