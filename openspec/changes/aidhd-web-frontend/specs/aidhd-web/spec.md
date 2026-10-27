# Spec Delta

## Purpose

Gives `aidhd` a browser view of task state and a few click actions over a loopback-only HTTP server, so a user can see what is scheduled without parsing CLI output.

## ADDED Requirements

### Requirement: Loopback-only server on a non-common port

`aidhd serve` SHALL start an HTTP server bound to `127.0.0.1` and SHALL NOT accept connections from other interfaces. The default port SHALL be `7317`, overridable with `--port`. On startup the server SHALL print the URL it is listening on to stderr and SHALL keep running until interrupted.

#### Scenario: Default port

- **WHEN** the user runs `aidhd serve`
- **THEN** the server listens on `127.0.0.1:7317` and stderr shows the URL

#### Scenario: Explicit port

- **WHEN** the user runs `aidhd serve --port 9123`
- **THEN** the server listens on `127.0.0.1:9123`

#### Scenario: Port already in use

- **WHEN** the requested port is already bound by another process
- **THEN** the command exits non-zero with an error naming the port

### Requirement: Self-contained dashboard page

`GET /` SHALL return `200 OK` with `Content-Type: text/html` and a single HTML document whose CSS and JavaScript are inline. The page SHALL render without loading any external script, stylesheet, font, or image, so it works offline. It SHALL display pending tasks and task counts, and SHALL offer controls to add a task, mark one done, and run a secretary pass.

#### Scenario: Page loads offline

- **WHEN** a browser requests `GET /` while offline
- **THEN** the page renders and its controls work without any network request beyond the server itself

#### Scenario: Page shows current tasks

- **WHEN** the page loads and there are pending tasks
- **THEN** each task's title, priority, and due date are visible

### Requirement: Mobile-first responsive layout

The dashboard SHALL be built mobile-first. It SHALL declare a viewport meta tag, keep a single readable column at phone widths with no horizontal scrolling at 360px, and size every interactive control to at least 44x44 CSS pixels for touch. No action SHALL depend on hover. At wider viewports the content SHALL expand to a centered maximum width without changing which controls are available.

#### Scenario: Phone viewport

- **WHEN** the page is opened on a 360px-wide viewport
- **THEN** there is no horizontal scrollbar and every button is at least 44px in both dimensions

#### Scenario: Desktop viewport

- **WHEN** the page is opened on a wide viewport
- **THEN** the content is centered at a readable maximum width and every control remains reachable

### Requirement: JSON read endpoints for task state

The server SHALL expose read-only JSON endpoints: `GET /api/tasks` for pending tasks, `GET /api/stats` for counts by status, `GET /api/decisions?limit=N` for the most recent decisions, and `GET /api/next` for the most urgent open task. Each SHALL return `200 OK` with `application/json` and a JSON body. `GET /api/next` SHALL return JSON `null` when no open task exists. `GET /api/decisions` SHALL default to 20 entries when `limit` is absent.

#### Scenario: List pending tasks

- **WHEN** the client requests `GET /api/tasks`
- **THEN** the response is a JSON array of pending tasks with id, title, priority, and due date

#### Scenario: No open tasks

- **WHEN** the client requests `GET /api/next` and no task is open
- **THEN** the response body is JSON `null` with `200 OK`

#### Scenario: Decision limit

- **WHEN** the client requests `GET /api/decisions?limit=5`
- **THEN** the response contains at most five decisions, newest first

### Requirement: JSON write endpoints for basic actions

The server SHALL accept `POST /api/tasks` with a JSON body containing at least `title` to create a task, `POST /api/tasks/{id}/done` to mark a task complete, and `POST /api/secretary` with an optional `dry_run` boolean to run one secretary pass. Successful writes SHALL return `200 OK` with a JSON body describing the result. A malformed or missing JSON body SHALL return `400` and SHALL NOT change stored data.

#### Scenario: Add a task from the page

- **WHEN** the client posts `{"title": "buy milk"}` to `/api/tasks`
- **THEN** a pending task named "buy milk" is created and returned in the response

#### Scenario: Mark a task done

- **WHEN** the client posts to `/api/tasks/{id}/done` for a pending task
- **THEN** that task's status becomes completed and the response confirms it

#### Scenario: Dry-run secretary pass

- **WHEN** the client posts `{"dry_run": true}` to `/api/secretary`
- **THEN** decisions are computed and returned but no task is modified

#### Scenario: Invalid body

- **WHEN** the client posts a body that is not valid JSON to `/api/tasks`
- **THEN** the server returns `400` with a JSON error and creates no task

### Requirement: JSON errors and unknown routes

The server SHALL return errors as JSON objects with an `error` string field. An unknown path SHALL return `404`. An unknown task id SHALL return `404`. An internal failure SHALL return `500`. Every error response SHALL carry `Content-Type: application/json`.

#### Scenario: Unknown route

- **WHEN** the client requests `GET /nope`
- **THEN** the server returns `404` with a JSON error field

#### Scenario: Unknown task

- **WHEN** the client posts to `/api/tasks/does-not-exist/done`
- **THEN** the server returns `404` with a JSON error field
