#!/bin/sh
# Container entrypoint: the gateway plus the services bundled from services/.
#
# Each bundled service keeps its own listener; the gateway publishes the public
# paths of the services it actually started (see router/service_proxy.go). When
# a service cannot start, its routes stay on the gateway instead of returning a
# proxy error, so a partially configured deployment still serves the dashboard.
set -eu

BIN_DIR="${SERVICES_BIN_DIR:-/usr/local/bin}"
GATEWAY_BIN="${GATEWAY_BIN:-/new-api}"
DATA_DIR="${SERVICES_DATA_DIR:-/data}"
MAX_ATTEMPTS="${SERVICES_MAX_RESTART_ATTEMPTS:-5}"
MAX_BACKOFF_SECONDS=60
READY_TIMEOUT_SECONDS="${SERVICES_READY_TIMEOUT_SECONDS:-30}"

log() { printf '[bundled-services] %s\n' "$*"; }

value_of() { printenv "$1" 2>/dev/null || true; }

mkdir -p "$DATA_DIR" 2>/dev/null || true

# The bundled services load $DATA_DIR/.env themselves, but this script decides
# which of them start, so it has to see the same file. A variable that already
# exists in the process environment is kept, matching the gateway's own loading.
load_env_file() {
	file="$DATA_DIR/.env"
	[ -f "$file" ] || return 0
	loaded=0
	while IFS= read -r line || [ -n "$line" ]; do
		case "$line" in
		'' | '#'*) continue ;;
		esac
		line="${line#export }"
		case "$line" in
		*=*) ;;
		*) continue ;;
		esac
		key="$(printf '%s' "${line%%=*}" | tr -d '[:space:]')"
		case "$key" in
		'' | [!A-Za-z_]*) continue ;;
		esac
		value="$(printf '%s' "${line#*=}" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')"
		case "$value" in
		'"'*'"') value="${value#\"}" && value="${value%\"}" ;;
		"'"*"'") value="${value#\'}" && value="${value%\'}" ;;
		esac
		eval "current=\${$key-}"
		if [ -n "$current" ]; then
			continue
		fi
		eval "export $key=\"\$value\""
		loaded=$((loaded + 1))
	done <"$file"
	if [ "$loaded" -gt 0 ]; then
		log "loaded $loaded value(s) from $file"
	fi
}
load_env_file

# Every bundled service listens on a loopback port next to the gateway, so a
# deployment that changes PORT keeps working without restating service
# addresses; the *_LISTEN_ADDR variables still override each one.
PORT="${PORT:-3000}"
case "$PORT" in
'' | *[!0-9]*) PORT=3000 ;;
esac
export PORT
service_port() { printf '%s' "$((PORT + $1))"; }

# invitations needs the new-api database and its own tables, both over
# PostgreSQL, and SQL_DSN already holds a PostgreSQL URL. A deployment that
# only sets SQL_DSN therefore needs no separate database variable.
sql_dsn="$(value_of SQL_DSN)"
case "$sql_dsn" in
postgres://* | postgresql://*) ;;
*) sql_dsn="" ;;
esac
if [ -z "$(value_of NEWAPI_DATABASE_URL)" ] && [ -n "$sql_dsn" ]; then
	export NEWAPI_DATABASE_URL="$sql_dsn"
fi
if [ -z "$(value_of INVITATIONS_DATABASE_URL)" ]; then
	invitations_db="$(value_of DATABASE_URL)"
	case "$invitations_db" in
	postgres://* | postgresql://*) ;;
	*) invitations_db="$(value_of NEWAPI_DATABASE_URL)" ;;
	esac
	if [ -n "$invitations_db" ]; then
		export INVITATIONS_DATABASE_URL="$invitations_db"
		if [ "$invitations_db" = "$(value_of NEWAPI_DATABASE_URL)" ]; then
			log "invitations keeps its tables in the new-api database; set INVITATIONS_DATABASE_URL to give it its own"
		fi
	fi
fi

# wait_ready returns as soon as the service answers HTTP, so the first proxied
# request is not a race against the service's startup.
wait_ready() {
	name="$1"
	address="$2"
	path="$3"
	if ! command -v wget >/dev/null 2>&1; then
		log "wget is not available; not probing $name"
		return 0
	fi
	waited=0
	while [ "$waited" -lt "$READY_TIMEOUT_SECONDS" ]; do
		if wget -q -S -O /dev/null -T 2 "http://$address$path" 2>&1 | grep -q 'HTTP/'; then
			log "$name is listening on $address"
			return 0
		fi
		sleep 1
		waited=$((waited + 1))
	done
	log "$name did not answer within ${READY_TIMEOUT_SECONDS}s; the gateway reports 502 until it does"
	return 0
}

# The services read overlapping variable names (DATABASE_URL, LISTEN_ADDR,
# BASE_PATH) with different meanings, so this container maps its per-service
# variables onto each service here instead of letting one value leak into two
# processes. Every value still falls back to the name the standalone service
# documented, so an environment migrated from the separate deployments keeps
# working as long as the shared names do not disagree.
run_ai_bridge() {
	export AI_BRIDGE_LISTEN_ADDR="${AI_BRIDGE_LISTEN_ADDR:-127.0.0.1:$(service_port 1)}"
	export AI_BRIDGE_ADMIN_CONFIG_FILE="${AI_BRIDGE_ADMIN_CONFIG_FILE:-$DATA_DIR/ai-bridge/admin-config.env}"
	# ai-bridge forwards to the gateway next to it, so its upstream follows PORT.
	export AI_BRIDGE_UPSTREAM_BASE_URL="${AI_BRIDGE_UPSTREAM_BASE_URL:-http://127.0.0.1:$PORT}"
	export AI_BRIDGE_CYBER_POLICY_BLACKLIST_FILE="${AI_BRIDGE_CYBER_POLICY_BLACKLIST_FILE:-$DATA_DIR/ai-bridge/black.txt}"
	export AI_BRIDGE_CYBER_POLICY_BLACK_LOG_FILE="${AI_BRIDGE_CYBER_POLICY_BLACK_LOG_FILE:-$DATA_DIR/ai-bridge/black_log}"
	export AI_BRIDGE_CYBER_POLICY_IP_BLACKLIST_FILE="${AI_BRIDGE_CYBER_POLICY_IP_BLACKLIST_FILE:-$DATA_DIR/ai-bridge/black_ip}"
	export AI_BRIDGE_CYBER_POLICY_WHITELIST_FILE="${AI_BRIDGE_CYBER_POLICY_WHITELIST_FILE:-$DATA_DIR/ai-bridge/white.txt}"
	export AI_BRIDGE_CYBER_POLICY_WHITE_LOG_FILE="${AI_BRIDGE_CYBER_POLICY_WHITE_LOG_FILE:-$DATA_DIR/ai-bridge/white_log}"
	exec "$BIN_DIR/ai-bridge"
}

# invitations serves its API under /api, which is where the gateway forwards
# /invitations/api/* to. NEW_API_BASE_URL is what it calls back on, so it
# follows PORT like ai-bridge's upstream does.
run_invitations() {
	export LISTEN_ADDR="${INVITATIONS_LISTEN_ADDR:-127.0.0.1:$(service_port 2)}"
	export DATABASE_URL="${INVITATIONS_DATABASE_URL:-}"
	export NEW_API_BASE_URL="${INVITATIONS_NEW_API_BASE_URL:-${NEW_API_BASE_URL:-http://127.0.0.1:$PORT}}"
	export BASE_PATH="${INVITATIONS_BASE_PATH:-}"
	exec "$BIN_DIR/invitations"
}

# connect needs the public prefix it is mounted under, so its OAuth2 endpoints
# stay under /connect.
run_connect() {
	export LISTEN_ADDR="${CONNECT_LISTEN_ADDR:-127.0.0.1:$(service_port 3)}"
	export DATABASE_URL="${CONNECT_DATABASE_URL:-sqlite:connect.db?mode=rwc}"
	export API_BASE_URL="${CONNECT_API_BASE_URL:-http://127.0.0.1:$PORT}"
	export BASE_PATH="${CONNECT_BASE_PATH:-/connect}"
	exec "$BIN_DIR/connect"
}

# port_of extracts the port from a host:port listen address.
port_of() { printf '%s' "${1##*:}"; }

# supervise keeps one bundled service running in its own background job.
supervise() {
	name="$1"
	address="$2"
	path="$3"
	command="$4"
	attempts=0
	backoff=2
	stopping=0
	trap 'stopping=1; kill "${child:-0}" 2>/dev/null' TERM INT
	while :; do
		"$command" &
		child=$!
		log "started $name (pid $child)"
		wait_ready "$name" "$address" "$path"
		set +e
		wait "$child"
		status=$?
		set -e
		if [ "$stopping" -ne 0 ]; then
			log "$name stopped"
			return 0
		fi
		attempts=$((attempts + 1))
		if [ "$attempts" -ge "$MAX_ATTEMPTS" ]; then
			log "$name exited $attempts times (last status $status); leaving it stopped"
			return 0
		fi
		log "$name exited with status $status; restarting in ${backoff}s"
		sleep "$backoff"
		backoff=$((backoff * 2))
		[ "$backoff" -le "$MAX_BACKOFF_SECONDS" ] || backoff=$MAX_BACKOFF_SECONDS
	done
}

supervisors=""
if [ -z "$(value_of SESSION_SECRET)" ] && { [ -x "$BIN_DIR/connect" ] || [ -x "$BIN_DIR/ai-bridge" ]; }; then
	log "SESSION_SECRET is not set: the bundled services cannot decode the dashboard session, so /connect and /ai-bridge answer unauthorized. Set one SESSION_SECRET for the gateway and the services, in the container environment or in $DATA_DIR/.env."
fi

start_service() {
	name="$1"
	address="$2"
	path="$3"
	command="$4"
	supervise "$name" "$address" "$path" "$command" &
	supervisors="$supervisors $!"
}

if [ "$(value_of AI_BRIDGE_AUTOSTART)" != "false" ] && [ -x "$BIN_DIR/ai-bridge" ]; then
	mkdir -p "$DATA_DIR/ai-bridge" 2>/dev/null || true
	ai_bridge_listen="$(value_of AI_BRIDGE_LISTEN_ADDR)"
	ai_bridge_listen="${ai_bridge_listen:-127.0.0.1:$(service_port 1)}"
	export AI_BRIDGE_LISTEN_ADDR="$ai_bridge_listen"
	start_service ai-bridge "127.0.0.1:$(port_of "$ai_bridge_listen")" /ai-bridge run_ai_bridge
	export AI_BRIDGE_SERVICE_URL="${AI_BRIDGE_SERVICE_URL:-http://127.0.0.1:$(port_of "$ai_bridge_listen")}"
fi

if [ "$(value_of INVITATIONS_AUTOSTART)" != "false" ] && [ -x "$BIN_DIR/invitations" ]; then
	if [ -n "$(value_of INVITATIONS_DATABASE_URL)" ] && [ -n "$(value_of NEWAPI_DATABASE_URL)" ] && [ -n "$(value_of SESSION_SECRET)" ]; then
		invitations_listen="$(value_of INVITATIONS_LISTEN_ADDR)"
		invitations_listen="${invitations_listen:-127.0.0.1:$(service_port 2)}"
		export INVITATIONS_LISTEN_ADDR="$invitations_listen"
		start_service invitations "127.0.0.1:$(port_of "$invitations_listen")" /api/ run_invitations
		export INVITATIONS_SERVICE_URL="${INVITATIONS_SERVICE_URL:-http://127.0.0.1:$(port_of "$invitations_listen")}"
	else
		log "not starting invitations: it needs SESSION_SECRET and a PostgreSQL database - set SQL_DSN (reused for NEWAPI_DATABASE_URL), or NEWAPI_DATABASE_URL plus INVITATIONS_DATABASE_URL"
		unset INVITATIONS_SERVICE_URL 2>/dev/null || true
	fi
fi

if [ "$(value_of CONNECT_AUTOSTART)" != "false" ] && [ -x "$BIN_DIR/connect" ]; then
	connect_db="$(value_of CONNECT_DATABASE_URL)"
	if [ -z "$connect_db" ]; then
		case "$(value_of DATABASE_URL)" in
		sqlite:*) connect_db="$(value_of DATABASE_URL)" ;;
		esac
	fi
	connect_db="${connect_db:-sqlite:connect.db?mode=rwc}"
	case "$connect_db" in
	sqlite:*) ;;
	*)
		log "not starting connect: it only supports SQLite, but CONNECT_DATABASE_URL is not a sqlite: DSN. Set CONNECT_DATABASE_URL=sqlite:connect.db?mode=rwc; a shared DATABASE_URL belongs to invitations."
		unset CONNECT_SERVICE_URL 2>/dev/null || true
		connect_db=""
		;;
	esac
	if [ -n "$connect_db" ]; then
		if [ -n "$(value_of SESSION_SECRET)" ] && [ -n "$(value_of ISSUER_URL)" ]; then
			connect_listen="$(value_of CONNECT_LISTEN_ADDR)"
			connect_listen="${connect_listen:-127.0.0.1:$(service_port 3)}"
			export CONNECT_LISTEN_ADDR="$connect_listen"
			export CONNECT_DATABASE_URL="$connect_db"
			start_service connect "127.0.0.1:$(port_of "$connect_listen")" /connect/ run_connect
			export CONNECT_SERVICE_URL="${CONNECT_SERVICE_URL:-http://127.0.0.1:$(port_of "$connect_listen")}"
		else
			log "not starting connect: SESSION_SECRET and ISSUER_URL are required (ISSUER_URL is the origin clients reach, for example https://example.com)"
			unset CONNECT_SERVICE_URL 2>/dev/null || true
		fi
	fi
fi

"$GATEWAY_BIN" "$@" &
gateway=$!
trap 'kill "$gateway" $supervisors 2>/dev/null' TERM INT

set +e
wait "$gateway"
status=$?
set -e

log "gateway exited with status $status; stopping bundled services"
kill $supervisors 2>/dev/null || true
exit "$status"
