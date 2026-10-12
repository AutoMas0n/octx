#!/usr/bin/env python3
"""Harvest a Qobuz authentication token for `octx jam`.

Why this exists
---------------
Qobuz has no token refresh, and there is no working browser-free login. The
sign-in form is a plain POST, guarded by an invisible Google reCAPTCHA and a
CSRF token, which is why replaying the API's `user/login` call returns the
same opaque 401 for a real account and for a nonexistent one alike.

What does work is a browser that is genuinely signed in -- after which every
request to `www.qobuz.com/api.json/*` carries the session in the
`X-User-Auth-Token` request header.

Page JavaScript cannot read its own outgoing request headers, which is why
tools like the BlendUz browser extension exist. This script plays the same
part from outside the browser: it drives a real login with Playwright and
reads the header off a request the page itself made and the server answered
with a 2xx. It never replays a login request, and it never prints the token
unless you ask it to.

The browser profile is persistent, so a run reuses an existing session and
only signs in again once that session has died. Because the reCAPTCHA issues
no token to a headless browser, signing in needs a visible one: use
`harvest-docker.sh`, which supplies a virtual display, or pass `--headful`
on a box that has one.

Usage
-----
    ./harvest-docker.sh --account jesse                  # Docker, nothing installed
    ./qobuz-token-harvest.py --headful --account jesse    # Playwright installed locally

Sign-in credentials are read from the `qobuz` object of octx's `jam`
credential document (`accounts.<name>.qobuz.email` / `.password`), falling
back to `QOBUZ_EMAIL` / `QOBUZ_PASSWORD`. The harvested token is merged back
into that same document with `octx creds add jam`.

Scheduling
----------
    17 4 * * * /path/to/harvest-docker.sh --account jesse >>~/qobuz-harvest.log 2>&1

The first run signs in and leaves a session in the browser profile; later
runs reuse it and normally need no sign-in at all. Exit status is 0 when a
token was harvested and 1 otherwise, so the schedule can tell an expired
session from a successful run. A 1 means a human has to sign in again, which
needs a visible browser.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

HOST = "jam"
LOGIN_URL = "https://play.qobuz.com/login"
DISCOVER_URL = "https://play.qobuz.com/"
API_MARK = "api.json"
TOKEN_HEADER = "x-user-auth-token"
# The OAuth callback carries the literal string `undefined` in this header and
# still answers 200, because it does not check it. A real token is far longer,
# so anything short is discarded rather than stored as a credential.
MIN_TOKEN_LEN = 20
DEFAULT_PROFILE = Path.home() / ".local" / "share" / "octx" / "qobuz-profile"
# The web player's application id, used to ask Qobuz who a token belongs to.
# An account may override it with `accounts.<name>.qobuz.app_id`.
APP_ID = "798273057"
API_BASE = "https://www.qobuz.com/api.json/0.2/"

# play.qobuz.com opens on a call-to-action and hands off to the shop's
# server-rendered sign-in form. Both are rendered client-side, so every lookup
# tries a spread of selectors and gives up quietly; `--dump-dom` exists to fix
# them against a real page.
LOGIN_CTA = (
    "button:has-text('Log in')",
    "button:has-text('Sign in')",
    "a:has-text('Log in')",
)
EMAIL_FIELD = (
    "input#_username",
    "input[name=_username]",
    "input[type=email]",
    "input[name=email]",
    "input#email",
    "input[name=login]",
    "input[placeholder*='mail' i]",
)
PASSWORD_FIELD = (
    "input#_password",
    "input[name=_password]",
    "input[type=password]",
    "input[autocomplete=current-password]",
)
SUBMIT = (
    # The submit control is `button#login`. A text match is not safe here:
    # "Sign in with Google" precedes it in the DOM and `has-text` is a
    # substring match, so it would win and start an OAuth flow instead.
    "button#login",
    "button[type=submit]",
    "input[type=submit]",
)
CONSENT = (
    "#didomi-notice-agree-button",
    "button:has-text('Accept all')",
    "button:has-text('I agree')",
    "button:has-text('Tout accepter')",
)


def log(message: str) -> None:
    """Write a diagnostic to stderr; stdout is reserved for the token."""
    print(f"[harvest] {message}", file=sys.stderr, flush=True)


def merge_token(document: str | None, account: str, token: str) -> str:
    """Return a minified `jam` document with `account`'s Qobuz token set.

    Only the one field is touched: everything else the document already
    carries -- the shared Last.fm key, the ListenBrainz tokens -- survives.
    An unparseable document is refused rather than replaced, because it is
    the only copy of every other credential the arm reads.
    """
    if document is None or not document.strip():
        doc: dict = {"version": 1, "default": account, "accounts": {}}
    else:
        try:
            doc = json.loads(document)
        except json.JSONDecodeError as error:
            raise SystemExit(
                f"the credential stored for host `{HOST}` is not valid JSON "
                f"({error.msg} at line {error.lineno} column {error.colno})\n"
                f"  inspect it with `octx creds get {HOST} --raw`, then rewrite it "
                f"with `octx creds add {HOST}`"
            )
        if not isinstance(doc, dict):
            raise SystemExit(
                f"the credential stored for host `{HOST}` is not a jam account document\n"
                f"  replace it with `octx creds add {HOST}`"
            )

    doc.setdefault("version", 1)
    doc.setdefault("accounts", {})
    if not isinstance(doc["accounts"], dict):
        raise SystemExit(f"`accounts` in the credential for host `{HOST}` is not an object")

    account_doc = doc["accounts"].setdefault(account, {})
    qobuz = account_doc.setdefault("qobuz", {})
    qobuz["user_auth_token"] = token

    return json.dumps(doc, separators=(",", ":"))


def octx_read_document(octx: str) -> str | None:
    """Read the `jam` document, or None when no credential is stored yet."""
    done = subprocess.run(
        [octx, "creds", "get", HOST, "--raw"],
        capture_output=True,
        text=True,
    )
    if done.returncode != 0:
        return None
    return done.stdout.strip() or None


def octx_write_document(octx: str, document: str) -> None:
    """Store the document.

    The value travels in argv because `octx creds add` offers no stdin path,
    so it is briefly visible to processes of the same user on a machine where
    that matters.
    """
    done = subprocess.run(
        [octx, "creds", "add", HOST, "--token", document],
        capture_output=True,
        text=True,
    )
    if done.returncode != 0:
        raise SystemExit(f"`octx creds add {HOST}` failed: {done.stderr.strip()}")


def login_credentials(document: str | None, account: str) -> tuple[str | None, str | None]:
    """The Qobuz sign-in credentials, from the document and then the environment.

    The password is a field of the document rather than an argument so that it
    lives in the same encrypted store as everything else, and never in a
    process listing or a shell history.
    """
    email = password = None
    if document:
        try:
            doc = json.loads(document)
            qobuz = doc.get("accounts", {}).get(account, {}).get("qobuz", {})
            email = qobuz.get("email") or None
            password = qobuz.get("password") or None
        except (json.JSONDecodeError, AttributeError):
            pass
    return email or os.environ.get("QOBUZ_EMAIL"), password or os.environ.get("QOBUZ_PASSWORD")


def _visible(page, selectors, timeout_ms: int = 4000):
    """The first selector that becomes visible, or None."""
    deadline = time.monotonic() + timeout_ms / 1000
    while time.monotonic() < deadline:
        for selector in selectors:
            try:
                locator = page.locator(selector).first
                if locator.is_visible():
                    return locator
            except Exception:  # noqa: BLE001 - a bad selector is one to skip
                continue
        page.wait_for_timeout(200)
    return None


def _dismiss_consent(page) -> None:
    locator = _visible(page, CONSENT, timeout_ms=1500)
    if locator is None:
        return
    try:
        locator.click(timeout=2000)
        log("dismissed the consent banner")
    except Exception:  # noqa: BLE001 - the banner is optional
        pass


def _sign_in(page, email: str | None, password: str | None, manual: bool) -> None:
    """Drive the real login form so the page itself makes the login call.

    The player's login page is a call-to-action that hands off to the shop's
    server-rendered sign-in form: a plain form POST carrying a CSRF token.
    Clicking through keeps both the CSRF token and the redirect back to the
    player correct, which is why this is a click and never a replayed request.
    """
    if _visible(page, EMAIL_FIELD, timeout_ms=2000) is None:
        cta = _visible(page, LOGIN_CTA, timeout_ms=6000)
        if cta is not None:
            try:
                cta.click(timeout=5000)
                log("clicked the log-in button")
            except Exception:  # noqa: BLE001 - the form may already be open
                pass
            page.wait_for_timeout(5000)

    email_field = _visible(page, EMAIL_FIELD, timeout_ms=8000)
    if email_field is None or not email:
        log("no sign-in form found; waiting for a manual sign-in")
        return

    try:
        email_field.fill(email, timeout=5000)
        log("filled the email field")
    except Exception as error:  # noqa: BLE001
        log(f"could not fill the email field ({error.__class__.__name__})")
        return

    password_field = _visible(page, PASSWORD_FIELD, timeout_ms=5000)
    if password_field is None or not password:
        log("no password field found; finish the sign-in by hand")
        return

    try:
        password_field.fill(password, timeout=5000)
        log("filled the password field")
    except Exception as error:  # noqa: BLE001
        log(f"could not fill the password field ({error.__class__.__name__})")
        return

    submit = _visible(page, SUBMIT, timeout_ms=2000)
    try:
        if submit is not None:
            submit.click(timeout=3000)
        else:
            password_field.press("Enter")
        log("submitted the sign-in form")
    except Exception:  # noqa: BLE001 - Enter on the field is the fallback
        try:
            password_field.press("Enter")
        except Exception:  # noqa: BLE001
            pass

    if manual:
        log("finish any remaining prompt in the browser window")


def harvest(
    *,
    email: str | None,
    password: str | None,
    profile: Path,
    headless: bool,
    manual: bool,
    force_login: bool,
    timeout: float,
    dump_dom: Path | None,
) -> str | None:
    """Run the browser, log in if needed, and return the observed token."""
    try:
        from playwright.sync_api import sync_playwright
    except ImportError:
        raise SystemExit(
            "playwright is not installed\n"
            "  pip install playwright && playwright install chromium\n"
            "  (on a headless box, run this script under `xvfb-run` with --headful)"
        ) from None

    observed: dict[str, str] = {}

    def capture(response) -> None:
        if observed:
            return
        request = response.request
        if API_MARK not in request.url or not 200 <= response.status < 300:
            return
        try:
            headers = {name.lower(): value for name, value in request.headers.items()}
        except Exception:  # noqa: BLE001 - headers can be gone once disposed
            return
        token = headers.get(TOKEN_HEADER)
        if token and len(token) >= MIN_TOKEN_LEN:
            route = request.url.split(API_MARK, 1)[-1].split("?")[0]
            observed["token"] = token
            log(f"captured a {len(token)}-character token from a {response.status} answer to {route}")

    with sync_playwright() as playwright:
        context = playwright.chromium.launch_persistent_context(
            user_data_dir=str(profile),
            headless=headless,
            viewport={"width": 1440, "height": 900},
            args=["--disable-blink-features=AutomationControlled"],
        )
        context.on("response", capture)
        page = context.pages[0] if context.pages else context.new_page()

        if force_login:
            # The profile is dedicated to Qobuz, so dropping every cookie is
            # exactly "sign out": the player gets a login page to fill again.
            context.clear_cookies()
            log("cleared the stored session")

        log(f"opening {LOGIN_URL}")
        try:
            page.goto(LOGIN_URL, wait_until="domcontentloaded", timeout=60000)
        except Exception as error:  # noqa: BLE001
            log(f"could not open the login page ({error.__class__.__name__})")

        _dismiss_consent(page)

        # A live session answers this immediately, because the player calls
        # the API as soon as it loads. Only a dead one needs the form.
        page.wait_for_timeout(4000)
        if not observed:
            log("no session in this profile")
            if manual or email:
                _sign_in(page, email, password, manual)
            else:
                log(
                    "no credentials in the `jam` document and none in the environment "
                    "-- re-run with --headful and sign in by hand"
                )

        started = time.monotonic()
        deadline = started + timeout
        nudged = False
        while not observed and time.monotonic() < deadline:
            page.wait_for_timeout(1000)
            if not nudged and time.monotonic() - started > 15:
                nudged = True
                try:
                    page.goto(DISCOVER_URL, wait_until="domcontentloaded", timeout=15000)
                    log("moved to the player so it makes API calls")
                except Exception:  # noqa: BLE001
                    pass

        if not observed and dump_dom is not None:
            try:
                dump_dom.write_text(page.content(), encoding="utf-8")
                log(f"wrote the rendered page to {dump_dom} for selector debugging")
            except Exception as error:  # noqa: BLE001
                log(f"could not write {dump_dom} ({error.__class__.__name__})")

        context.close()

    return observed.get("token")


def app_id_from(document: str | None, account: str) -> str:
    """The Qobuz application id for an account, falling back to the player's."""
    if document:
        try:
            doc = json.loads(document)
        except json.JSONDecodeError:
            return APP_ID
        if isinstance(doc, dict):
            qobuz = doc.get("accounts", {}).get(account, {}).get("qobuz", {})
            if isinstance(qobuz, dict) and qobuz.get("app_id"):
                return str(qobuz["app_id"])
    return APP_ID


def qobuz_identity(token: str, app_id: str) -> dict | None:
    """Ask Qobuz who a token belongs to, or None when that cannot be told.

    A browser profile holds one signed-in session, so a token observed in it may
    belong to a different account than the one being refreshed. Storing that
    silently would point one person's playlist sync at another person's library,
    so the identity is confirmed before anything is written.
    """
    import urllib.error
    import urllib.request

    request = urllib.request.Request(f"{API_BASE}user/get?app_id={app_id}")
    request.add_header("X-App-Id", app_id)
    request.add_header("X-User-Auth-Token", token)
    try:
        with urllib.request.urlopen(request, timeout=30) as answer:
            body = json.loads(answer.read())
    except (urllib.error.URLError, OSError, ValueError):
        return None
    return body if isinstance(body, dict) else None


def self_test() -> int:
    """Check the one piece that touches the credential store."""
    created = merge_token(None, "jesse", "tok-1")
    doc = json.loads(created)
    assert doc["version"] == 1, doc
    assert doc["default"] == "jesse", doc
    assert doc["accounts"]["jesse"]["qobuz"]["user_auth_token"] == "tok-1", doc

    existing = json.dumps(
        {
            "version": 1,
            "default": "jesse",
            "lastfm": {"api_key": "shared"},
            "accounts": {
                "jesse": {
                    "lastfm": {"user": "jesse1819"},
                    "listenbrainz": {"user": "automason", "token": "lb"},
                    "qobuz": {"email": "j@example.com", "user_auth_token": "old"},
                }
            },
        }
    )
    merged = json.loads(merge_token(existing, "jesse", "tok-2"))
    assert merged["accounts"]["jesse"]["qobuz"]["user_auth_token"] == "tok-2", merged
    assert merged["accounts"]["jesse"]["qobuz"]["email"] == "j@example.com", merged
    assert merged["accounts"]["jesse"]["listenbrainz"]["token"] == "lb", merged
    assert merged["lastfm"]["api_key"] == "shared", merged

    added = json.loads(merge_token(existing, "wife", "tok-3"))
    assert added["accounts"]["wife"]["qobuz"]["user_auth_token"] == "tok-3", added
    assert added["accounts"]["jesse"]["qobuz"]["user_auth_token"] == "old", added

    try:
        merge_token("not json", "jesse", "tok")
    except SystemExit:
        pass
    else:
        raise AssertionError("a malformed document must be refused, not replaced")

    print("self-test ok")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--account", help="the jam account to refresh (default: the document's default)")
    parser.add_argument("--profile", type=Path, default=DEFAULT_PROFILE, help=f"browser profile (default: {DEFAULT_PROFILE})")
    parser.add_argument("--headful", action="store_true", help="allow waiting for a human to finish the sign-in")
    parser.add_argument("--headless", action="store_true", help="no visible browser: can reuse a session, cannot sign in")
    parser.add_argument("--login", action="store_true", help="discard the stored session and sign in again")
    parser.add_argument("--timeout", type=float, default=180.0, help="seconds to wait for the token (default: 180)")
    parser.add_argument("--octx", default="octx", help="the octx binary to drive (default: octx)")
    parser.add_argument("--print-token", action="store_true", help="print the token instead of storing it")
    parser.add_argument("--dump-dom", type=Path, help="write the rendered page here when no token is found")
    parser.add_argument("--self-test", action="store_true", help="check the document merge and exit")
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    if shutil.which(args.octx) is None and not args.print_token:
        raise SystemExit(f"`{args.octx}` is not on PATH; pass --octx, or use --print-token")

    document = None if args.print_token else octx_read_document(args.octx)
    account = args.account
    if account is None:
        try:
            account = (json.loads(document) if document else {}).get("default")
        except json.JSONDecodeError:
            account = None
        if account is None:
            raise SystemExit("pass --account <name>: the `jam` document names no default account")

    email, password = login_credentials(document, account)
    if email and password:
        log(f"using the stored sign-in for `{email}`")
    elif document is None:
        log(f"no `{HOST}` credential yet; a new one will be created for `{account}`")

    token = harvest(
        email=email,
        password=password,
        profile=args.profile,
        headless=args.headless and not args.headful,
        manual=args.headful,
        force_login=args.login,
        timeout=args.timeout,
        dump_dom=args.dump_dom,
    )

    if token is None:
        log("no token was observed")
        log(
            "a session that has expired can only be replaced by a visible browser: "
            "./harvest-docker.sh, or --headful where a display exists"
        )
        return 1

    expected = (email or "").strip()
    if expected:
        who = qobuz_identity(token, app_id_from(document, account))
        if who is None:
            log("could not confirm which account this token belongs to")
            log("leaving the stored token untouched rather than risk writing another account's")
            return 1
        actual = str(who.get("email") or who.get("login") or "").strip()
        if actual.lower() != expected.lower():
            log(f"refusing to store a token for `{actual}` under `{account}`, which is set up for `{expected}`")
            log("this profile holds a different account's session -- re-run with --login to sign in as the right one")
            return 1
        log(f"confirmed the token belongs to `{actual}`")

    if args.print_token:
        print(token)
        return 0

    octx_write_document(args.octx, merge_token(document, account, token))
    log(f"stored the token for `{account}` in host `{HOST}`")
    return 0


if __name__ == "__main__":
    sys.exit(main())