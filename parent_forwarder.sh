#!/usr/bin/env bash
# Run on the EC2 host alongside the chat-relayer enclave.
# Bridges VSOCK <-> TCP for every service chat-relayer needs.
#
# Usage:
#   ENCLAVE_CID=$(sudo nitro-cli describe-enclaves | jq -r '.[0].EnclaveCID')
#   ./parent_forwarder.sh
#
# Or source an env file first:
#   set -a; source .env.runtime; set +a
#   ./parent_forwarder.sh

set -euo pipefail

# ── Helpers ──────────────────────────────────────────────────────────────────
extract_host() { printf '%s' "$1" | sed -nE 's#.*@([^:/]+).*#\1#p'; }
extract_port() {
    local p
    p=$(printf '%s' "$1" | sed -nE 's#.*:([0-9]+)(/.*)?$#\1#p')
    printf '%s' "${p:-5432}"
}
url_host() { printf '%s' "$1" | sed -nE 's#https?://([^/:]+).*#\1#p'; }
url_port() {
    local p
    p=$(printf '%s' "$1" | sed -nE 's#https?://[^/:]+:?([0-9]*).*#\1#p')
    printf '%s' "${p:-443}"
}

# ── Discover enclave CID ──────────────────────────────────────────────────────
if [ -z "${ENCLAVE_CID:-}" ]; then
    ENCLAVE_CID=$(sudo nitro-cli describe-enclaves | jq -r '.[0].EnclaveCID // empty')
fi
if [ -z "${ENCLAVE_CID:-}" ]; then
    echo "ERROR: no running enclave found. Run: make run"
    exit 1
fi
echo "Enclave CID: ${ENCLAVE_CID}"

# ── Push config to enclave (VSOCK:7000) ──────────────────────────────────────
ENV_FILE="${ENV_FILE:-.env.runtime}"
if [ -f "${ENV_FILE}" ]; then
    echo "Pushing config from ${ENV_FILE} -> VSOCK:${ENCLAVE_CID}:7000"
    socat - "VSOCK-CONNECT:${ENCLAVE_CID}:7000" < "${ENV_FILE}"
else
    echo "WARNING: ${ENV_FILE} not found - enclave will use built-in defaults"
fi

# ── Inbound: external TCP -> enclave VSOCK ───────────────────────────────────
echo "TCP:4002 -> VSOCK:${ENCLAVE_CID}:4002  (HTTP API)"
socat TCP-LISTEN:4002,reuseaddr,fork \
    VSOCK-CONNECT:"${ENCLAVE_CID}":4002 &

# ── Outbound: enclave VSOCK -> external TCP ──────────────────────────────────
if [ -n "${DATABASE_URL:-}" ]; then
    PG_HOST=$(extract_host "${DATABASE_URL}")
    PG_PORT=$(extract_port "${DATABASE_URL}")
    echo "VSOCK:8101 -> ${PG_HOST}:${PG_PORT}  (Postgres)"
    socat VSOCK-LISTEN:8101,reuseaddr,fork \
        TCP:"${PG_HOST}":"${PG_PORT}" &
fi

if [ -n "${REDIS_URL:-}" ]; then
    REDIS_HOST=$(extract_host "${REDIS_URL}")
    REDIS_PORT=$(extract_port "${REDIS_URL}")
    echo "VSOCK:8102 -> ${REDIS_HOST}:${REDIS_PORT}  (Redis)"
    socat VSOCK-LISTEN:8102,reuseaddr,fork \
        TCP:"${REDIS_HOST}":"${REDIS_PORT}" &
fi

if [ -n "${SUI_RPC_URL:-}" ]; then
    H=$(url_host "${SUI_RPC_URL}"); P=$(url_port "${SUI_RPC_URL}")
    echo "VSOCK:8103 -> ${H}:${P}  (Sui RPC)"
    socat VSOCK-LISTEN:8103,reuseaddr,fork TCP:"${H}":"${P}" &
fi

if [ -n "${EMBEDDING_API_BASE:-}" ]; then
    H=$(url_host "${EMBEDDING_API_BASE}"); P=$(url_port "${EMBEDDING_API_BASE}")
    echo "VSOCK:8104 -> ${H}:${P}  (Embedding API)"
    socat VSOCK-LISTEN:8104,reuseaddr,fork TCP:"${H}":"${P}" &
fi

if [ -n "${WALRUS_PUBLISHER_URL:-}" ]; then
    H=$(url_host "${WALRUS_PUBLISHER_URL}"); P=$(url_port "${WALRUS_PUBLISHER_URL}")
    echo "VSOCK:8105 -> ${H}:${P}  (Walrus publisher)"
    socat VSOCK-LISTEN:8105,reuseaddr,fork TCP:"${H}":"${P}" &
fi

if [ -n "${WALRUS_AGGREGATOR_URL:-}" ]; then
    H=$(url_host "${WALRUS_AGGREGATOR_URL}"); P=$(url_port "${WALRUS_AGGREGATOR_URL}")
    echo "VSOCK:8106 -> ${H}:${P}  (Walrus aggregator)"
    socat VSOCK-LISTEN:8106,reuseaddr,fork TCP:"${H}":"${P}" &
fi

if [ -n "${PINAIVU_API_BASE:-}" ]; then
    H=$(url_host "${PINAIVU_API_BASE}"); P=$(url_port "${PINAIVU_API_BASE}")
    echo "VSOCK:8107 -> ${H}:${P}  (pinaivu-api / coordinator)"
    socat VSOCK-LISTEN:8107,reuseaddr,fork TCP:"${H}":"${P}" &
fi

# ── Log collection ────────────────────────────────────────────────────────────
echo "VSOCK:5000 -> chat-relayer.log"
socat VSOCK-LISTEN:5000,reuseaddr,fork \
    OPEN:chat-relayer.log,creat,append &

echo ""
echo "All bridges active."
echo "Test: curl -k https://localhost:4002/health"
echo "Logs: tail -f chat-relayer.log"
echo ""

wait
