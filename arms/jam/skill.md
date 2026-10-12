# jam

## Description
Turns ListenBrainz recommendations into a playlist document — and, when asked, into an idempotent Qobuz playlist — and feeds listens from NDJSON, Last.fm, or ListenBrainz back into ListenBrainz. The read side is `recs`/`radio` → document → `json`/`qobuz`; the write side is `scrobble` → `json`/`listenbrainz`. It runs once per invocation and carries no scheduler.

The Qobuz target is **jam-owned**: the sink converges the playlist on the source's current output, so a track that is no longer recommended, a hand-added track, or a hand-reordering is removed or overwritten on the next run. Point `--playlist` at a dedicated playlist, not one you edit by hand.

## Usage
```
octx x jam recs   [--count N] [--playlist <name|id>] [--dry-run] --output <json|qobuz>
octx x jam radio  --prompt PROMPT --mode <easy|medium|hard> [--playlist <name|id>] [--dry-run] --output <json|qobuz>
octx x jam scrobble [--source <ndjson|lastfm|listenbrainz>] [--input <path|->] [--limit N] --output <json|listenbrainz>
octx x jam status
```

Global options apply to every subcommand: `--account <name>`, `--user <name>`, `--help`, `--version`.

## Options
### Read side (`recs`, `radio`)
- `--count N` — recordings to request from the collaborative-filtering endpoint (default 25, clamped to its maximum of 1000). `recs` only.
- `--prompt PROMPT` — ListenBrainz prompt language, for example `artist:(queen)` or `tag:(shoegaze)`. Required by `radio`.
- `--mode <easy|medium|hard>` — how adventurous the radio picks are. Required by `radio`.
- `--playlist <name|id>` — target Qobuz playlist. A name is resolved against the account's own playlists and created when missing; a numeric value is used as the playlist id without a lookup. Ignored by `--output json`.
- `--dry-run` — resolve and report the intended additions and removals without writing. `--output qobuz` only.
- `--output <json|qobuz>` — where the playlist document goes. Required.

### Write side (`scrobble`)
- `--source <ndjson|lastfm|listenbrainz>` — where the listens come from. Defaults to `ndjson`.
- `--input <path|->` — NDJSON file, or `-` for standard input. Required by `--source ndjson` and rejected for the networked sources.
- `--limit N` — how many listens to fetch from a networked source. `lastfm` and `listenbrainz` only.
- `--output <json|listenbrainz>` — where the listen document goes. Required. `json` writes it to stdout without contacting ListenBrainz.

### Global
- `--account <name>` — the account the run speaks as. Defaults to the document's `default`, or the sole account when exactly one exists. There is no fallback between accounts.
- `--user <name>` — the user name for the source that needs one, instead of the account's. A public `recs` read with `--user` needs no credential at all.
- `--help`, `--version` — standard.

## Examples
- `octx x jam recs --output json` — print the recommendations for the default account's ListenBrainz user; needs no Qobuz credential
- `octx x jam recs --count 50 --output qobuz` — sync 50 recommendations into the default `ListenBrainz Discovery` playlist
- `octx x jam radio --prompt "artist:(queen)" --mode hard --output qobuz --playlist "Radio Jams"` — fill `Radio Jams` from an LB Radio prompt
- `octx x jam recs --output qobuz --dry-run` — report what a sync would add and remove without touching the playlist
- `octx x jam scrobble --source lastfm --limit 200 --output listenbrainz --account wife` — send her Last.fm history to her ListenBrainz account
- `octx x jam scrobble --input history.ndjson --output listenbrainz` — submit listens from a file
- `octx x jam status` — check whether a `recs` run could succeed before scheduling one

## Credentials
One host, `jam`, holds a JSON account document. Add or replace it with:

```
octx creds add jam
```

The document's shape:

```json
{
  "version": 1,
  "default": "jesse",
  "lastfm": { "api_key": "<shared application key>" },
  "accounts": {
    "jesse": {
      "lastfm": { "user": "jesse1819" },
      "listenbrainz": { "user": "automason", "token": "<lb token>" },
      "qobuz": { "user_auth_token": "<qobuz token>", "app_id": "<optional override>" }
    }
  }
}
```

- `lastfm.api_key` is application-wide and shared by every account; `lastfm.user`, `listenbrainz.user`/`token`, and `qobuz.user_auth_token` belong to one account.
- `--account <name>` selects the entry; without it the document's `default` is used, or the sole account. Several accounts with no default, or an unknown name, fail while naming the accounts present.
- The Last.fm key lives in this same `jam` document — there is no separate host.
- Only the fields a run needs are read: `--output json` never reads the Qobuz fields, and a public `recs --user` read reads nothing at all. No credential value is ever printed.

## Prerequisites
- **The ListenBrainz account must already have scrobbles.** `recs` reads a collaborative-filtering model ListenBrainz trains from listens an external scrobbler submits; `jam` cannot create that input, so a user with no listening history has no recommendations until something scrobbles some. Run `octx x jam scrobble` first if needed. ListenBrainz only rebuilds that model on its own daily run (currently 02:01 UTC), so an account scrobbled since the last one keeps answering `204` for up to a day — that is the model catching up, not a fault.
- **A credential document at host `jam`** (above) with the account fields the run needs: `listenbrainz.user`/`token` for `recs`, `radio`, and `status`; `qobuz.user_auth_token` for `--output qobuz`; the shared `lastfm.api_key` (and a `lastfm.user`) for `--source lastfm`.
- **A Qobuz account** whose `user_auth_token` is in the document, for `--output qobuz`. The catalog and playlist endpoints are what the sink talks to.
- **Network access** to `api.listenbrainz.org`, `listenbrainz.org/player`, `www.qobuz.com`, and (for the Last.fm source) `ws.audioscrobbler.com`.
- **An external scheduler** (cron, a systemd timer, or `termux-job-scheduler`) for recurring runs. The arm runs once per invocation and carries no timer; schedule one command per account, one target playlist per line. Guard a scheduled `recs` with `octx x jam status` so it skips rather than fails while no model exists, and run the first one with `--dry-run`.

## Failure modes
- **No subcommand** — exits non-zero and prints usage listing `recs`, `radio`, `scrobble`, and `status`.
- **A document-producing subcommand without `--output`**, or with a sink invalid for it (for example `recs --output listenbrainz`) — exits non-zero before any request. `status` needs no sink.
- **The credential document is absent, malformed, or lacks a needed field** — exits non-zero naming the field's path (for example `accounts.wife.qobuz.user_auth_token`) and host `jam`, and never echoing a value. Add the field with `octx creds add jam`.
- **Several accounts and no `default`, or an unknown `--account`** — exits non-zero naming the accounts present; nothing is requested.
- **The recommendation endpoint answers `204`, or a source otherwise yields nothing** — exits non-zero with guidance and writes nothing. A source with nothing to give is never treated as an empty playlist, so no track is removed and no document reaches stdout. This is normal both for a user with no listening history yet and for one scrobbled since ListenBrainz's last daily model rebuild — check `octx x jam status` again after that run before concluding anything is wrong.
- **`radio` without `--prompt` or `--mode`** — exits non-zero naming the missing argument.
- **`--output qobuz` for an account with no `qobuz.user_auth_token`** — exits non-zero naming that field before any Qobuz request.
- **A Qobuz request is rate-limited (`429`)** — the arm backs off and retries; if retries are exhausted it exits non-zero reporting the Qobuz error. An entry whose catalog search fails is reported and skipped, and the rest of the run continues.
- **`--output listenbrainz` with a token that may not submit** — exits non-zero distinguishing a submit-permission failure from a missing credential; the token needs the `listenbrainz:submit-listens` scope.
- **NDJSON lines that fail to parse** — each is reported with its line number; the valid lines are still delivered, and the run exits non-zero to report the loss. Empty input exits zero.
- **`--source ndjson` without `--input`**, or `--input` with a networked source — exits non-zero naming the argument.
