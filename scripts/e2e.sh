#!/usr/bin/env bash
# End-to-end check: capture the traffic of the examples/ stack and assert on the exchanges.
# Needs Docker, jq, sudo (to grant capture capabilities) and a built binary.
#
# Usage: scripts/e2e.sh [path/to/tylomon]
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bin="$(realpath "${1:-$root/target/debug/tylomon}")"
compose=(docker compose -f "$root/examples/docker-compose.yml")
project="tylomon-example"
work="$(mktemp -d)"

cleanup() {
  "${compose[@]}" down --timeout 1 >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT

fail() {
  echo "FAIL: $*" >&2
  echo "--- captured lines: $(wc -l <"$work/out.jsonl")" >&2
  head -n 5 "$work/out.jsonl" >&2 || true
  echo "--- log" >&2
  tail -n 20 "$work/tylomon.log" >&2 || true
  exit 1
}

if ! getcap "$bin" | grep -q cap_net_raw; then
  sudo setcap cap_net_raw,cap_net_admin=eip "$bin"
fi

"${compose[@]}" up -d --wait

# SIGINT lets tylomon shut down cleanly and write the HAR file; timeout exits 124 for that.
timeout -s INT 15 "$bin" --no-tui --project "$project" \
  --har "$work/session.har" --write "$work/session.pcap" \
  --log-file "$work/tylomon.log" >"$work/out.jsonl" || [ $? -eq 124 ]

assert() {
  local description="$1" filter="$2"
  jq -s -e "any(.[]; $filter)" "$work/out.jsonl" >/dev/null || fail "$description"
  echo "ok: $description"
}

assert "poller -> gateway GET /api/health returns 200" \
  '.method == "GET" and .path == "/api/health" and .status == 200 and (.src | test("poller")) and (.dst | test("gateway"))'
assert "poller -> gateway POST /api/echo returns 200 with a body" \
  '.method == "POST" and .path == "/api/echo" and .status == 200 and .request_body.text != null and .response_body.text != null'
assert "gateway -> api hop captured on the second network" \
  '.path == "/health" and .status == 200 and (.src | test("gateway")) and (.dst | test("api"))'
assert "response times are measured" \
  '.duration_ms != null'

jq -e '.log.entries | length > 0' "$work/session.har" >/dev/null || fail "HAR file has no entries"
echo "ok: HAR export"

[ -s "$work/session.pcap" ] || fail "pcap recording is empty"
"$bin" --no-tui --read "$work/session.pcap" --log-file "$work/replay.log" \
  | jq -s -e 'any(.[]; .path == "/api/health" and .status == 200)' >/dev/null \
  || fail "replaying the recording shows no exchanges"
echo "ok: pcap record and replay"
