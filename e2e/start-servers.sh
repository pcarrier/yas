#!/usr/bin/env bash
# Starts a YAS server and YAS edge for e2e tests.
# The edge proxies to the server over a Unix socket.
# Exits when either process exits.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# Create a temp directory for the socket
TMPDIR_E2E="${YAS_E2E_TMPDIR:-$(mktemp -d)}"
export YAS_SOCK="${TMPDIR_E2E}/yas-test.sock"
# Persistent server state is single-writer. Keep the test server away from any
# developer server using the platform default databases.
export YAS_EXTENSION_PATH="${TMPDIR_E2E}/extensions.redb"
export YAS_KV_PATH="${TMPDIR_E2E}/kv.redb"

# The edge connects only to this fixed home socket. Give the home server's
# Relay catalogue one remote of its own instead of exposing the developer's
# real yas.remotes file (which would let a spec attach and drive those servers
# while it believes it is isolated behind YAS_SOCK).
export YAS_REMOTES="${TMPDIR_E2E}/yas.remotes"
printf 'test = socket:%s\n' "$YAS_SOCK" >"$YAS_REMOTES"

# Where a spec can find the server behind the edge it is driving.  Playwright
# starts this script as its own process tree, so an exported YAS_SOCK reaches
# the edge and nothing else — a spec that shells out to the CLI would
# otherwise resolve the *default* socket and quietly interrogate a different
# server.  The file exists only while these servers do, so its absence
# correctly means "somebody else's edge, use the CLI's own resolution".
SOCK_HANDOFF="${REPO_ROOT}/e2e/.e2e-socket"
printf '%s' "$YAS_SOCK" >"$SOCK_HANDOFF"

# Where the muster supervisor looks for units, if a spec installs it.  It is
# resolved from the *server's* environment (the extension asks for it over the
# env family), so it has to be set here rather than by the spec — and it has to
# be an empty directory of our own, because the default is the developer's real
# one and a spec that started those units would be starting their work.
export YAS_MUSTER_DIR="${TMPDIR_E2E}/muster"
mkdir -p "$YAS_MUSTER_DIR"
MUSTER_HANDOFF="${REPO_ROOT}/e2e/.e2e-muster-dir"
printf '%s' "$YAS_MUSTER_DIR" >"$MUSTER_HANDOFF"

SERVER_PID=""
EDGE_PID=""
cleanup() {
    # Kill child processes
    if [ -n "$SERVER_PID" ]; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
    if [ -n "$EDGE_PID" ]; then
        kill "$EDGE_PID" 2>/dev/null || true
        wait "$EDGE_PID" 2>/dev/null || true
    fi
    rm -f "$SOCK_HANDOFF" "$MUSTER_HANDOFF"
    rm -rf "$TMPDIR_E2E"
}
trap cleanup EXIT INT TERM

# Start yas server. The Muster spec installs a persistent extension: a
# transient `ext run` ends with the CLI connection that started it, so it is no
# use to a spec that wants an extension still serving when the browser looks.
# None of the Playwright specs starts a real Wayland client. Avoid making this
# browser/edge suite depend on host GPU initialization; compositor-specific
# repros can opt back in with YAS_SKIP_COMPOSITOR=0.
export YAS_SKIP_COMPOSITOR="${YAS_SKIP_COMPOSITOR:-1}"
"${REPO_ROOT}/target/debug/yas" server &
SERVER_PID=$!

# Leave enough of Playwright's 30-second web-server budget for the edge, and
# report an early server exit instead of misdiagnosing it as a socket timeout.
for _ in $(seq 1 200); do
    if [ -S "$YAS_SOCK" ]; then
        break
    fi
    if ! kill -0 "$SERVER_PID" 2>/dev/null; then
        if wait "$SERVER_PID"; then
            SERVER_STATUS=0
        else
            SERVER_STATUS=$?
        fi
        SERVER_PID=""
        echo "ERROR: yas server exited before creating $YAS_SOCK (status=$SERVER_STATUS)" >&2
        exit 1
    fi
    sleep 0.1
done

if [ ! -S "$YAS_SOCK" ]; then
    echo "ERROR: yas server socket did not appear at $YAS_SOCK" >&2
    exit 1
fi

echo "yas server started (pid=$SERVER_PID, socket=$YAS_SOCK)"

# Start the YAS edge.
export YAS_PASSPHRASE="${YAS_PASSPHRASE:-test-secret}"
export YAS_ADDR="${YAS_ADDR:-127.0.0.1:3274}"
"${REPO_ROOT}/target/debug/yas" edge &
EDGE_PID=$!

echo "yas edge started (pid=$EDGE_PID, addr=$YAS_ADDR)"
echo "READY"

# Exit when either owned child exits; the EXIT trap cleans up the other.
wait -n "$SERVER_PID" "$EDGE_PID"
