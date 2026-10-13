#!/usr/bin/env bash
# Run the Qobuz token harvester in Docker.
#
# Chromium and its dependencies stay inside the container, so this works on a
# box with only Docker and no way to install packages — no root, no sudo, and
# nothing added to the host. Arguments are passed straight to the harvester:
#
#   ./harvest-docker.sh --account jesse
#
# The browser is headful on a virtual display on purpose. Qobuz's sign-in form
# is protected by an invisible Google reCAPTCHA, which issues no token to a
# headless browser, so a headless run can reuse a live session but can never
# sign one in. Xvfb is started by hand because `xvfb-run -a` hangs in the
# Playwright image.
set -euo pipefail

IMAGE="${HARVEST_IMAGE:-octx-qobuz-harvest:1.63.0}"
TOOLS="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUT="${HARVEST_OUT:-${HOME}/qobuz-harvest}"
DISPLAY_NUM="${HARVEST_DISPLAY:-:99}"

# One browser profile per account. A profile holds a single signed-in Qobuz
# session, so a shared one would let `--account sam` reuse Jesse's session and
# store *his* token under her name. Separate directories keep the sessions apart.
ACCOUNT=""
prev=""
for arg in "$@"; do
    [ "$prev" = "--account" ] && ACCOUNT="$arg"
    case "$arg" in --account=*) ACCOUNT="${arg#--account=}" ;; esac
    prev="$arg"
done
PROFILE_DIR="${HARVEST_PROFILE_DIR:-${HOME}/.local/share/octx-qobuz-profiles/${ACCOUNT:-default}}"

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
    echo "building $IMAGE (first run only)..." >&2
    docker build -t "$IMAGE" -f "${TOOLS}/Dockerfile.harvest" "$TOOLS" >&2
fi

mkdir -p "$OUT"

# The browser profile lives in a host directory, not a named volume. A volume
# is invisible to the host and unattached ~23h a day, so a `docker system
# prune --volumes` anywhere on this box would silently delete a signed-in
# Qobuz session — and re-signing-in means beating a Google reCAPTCHA. A bind
# mount cannot be pruned. The trade-off is root-owned files in $PROFILE_DIR,
# which is fine: only the container ever reads them.
mkdir -p "$PROFILE_DIR"
# No --name: two accounts are refreshed from the same crontab, and a fixed
# name means the second `docker run` dies with "name already in use". Docker's
# generated name is unique, and --rm cleans up regardless.
exec docker run --rm \
    --shm-size=1g \
    -e "DISPLAY=${DISPLAY_NUM}" \
    -v "${HOME}/.config/octx:/root/.config/octx" \
    -v "/etc/machine-id:/etc/machine-id:ro" \
    -v "${HOME}/.local/bin/octx:/usr/local/bin/octx:ro" \
    -v "${TOOLS}:/work:ro" \
    -v "${OUT}:/out" \
    -v "${PROFILE_DIR}:/profile" \
    "$IMAGE" \
    bash -c '
        Xvfb '"${DISPLAY_NUM}"' -screen 0 1440x900x24 -nolisten tcp >/tmp/xvfb.log 2>&1 &
        XPID=$!
        sleep 3
        python3 -u /work/qobuz-token-harvest.py --headful --profile /profile "$@"
        RC=$?
        kill $XPID 2>/dev/null || true
        exit $RC
    ' _ "$@"