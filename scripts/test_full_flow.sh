#!/usr/bin/env bash
set -euo pipefail

CONTAINER_NAME="${CONTAINER_NAME:-qb-postgres-e2e}"
POSTGRES_IMAGE="${POSTGRES_IMAGE:-postgres:14.1}"
POSTGRES_PORT="${POSTGRES_PORT:-55433}"
API_PORT="${API_PORT:-18080}"
DB_URL="postgres://postgres:postgres@127.0.0.1:${POSTGRES_PORT}/qb"
BUNDLE_PATH="$(pwd)/samples/demo_bundle"
STATS_CSV="$(pwd)/samples/demo_bundle/stats/raw_scores.csv"
WORKBOOK_PATH="$(pwd)/samples/demo_bundle/score_workbooks/demo_score_index.xlsx"
WORKBOOK_ID="WB-CPHOS-18-DEMO-INDEX"
PAPER_ID="CPHOS-18-REGULAR-DEMO"

cleanup() {
  if [[ -n "${API_PID:-}" ]] && kill -0 "$API_PID" >/dev/null 2>&1; then
    kill "$API_PID" >/dev/null 2>&1 || true
    wait "$API_PID" >/dev/null 2>&1 || true
  fi
  if docker ps -a --format '{{.Names}}' | grep -qx "$CONTAINER_NAME"; then
    docker rm -f "$CONTAINER_NAME" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

echo "[1/7] Start PostgreSQL container"
if docker ps -a --format '{{.Names}}' | grep -qx "$CONTAINER_NAME"; then
  docker rm -f "$CONTAINER_NAME" >/dev/null
fi
docker run -d --name "$CONTAINER_NAME" \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=qb \
  -p "${POSTGRES_PORT}:5432" \
  "$POSTGRES_IMAGE" >/dev/null

for _ in $(seq 1 60); do
  if docker exec "$CONTAINER_NAME" pg_isready -U postgres -d qb >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

docker exec "$CONTAINER_NAME" pg_isready -U postgres -d qb >/dev/null

echo "[2/7] Apply migration"
docker exec -i "$CONTAINER_NAME" psql -U postgres -d qb < migrations/0001_init_pg.sql >/dev/null

echo "[3/7] Start API"
QB_DATABASE_URL="$DB_URL" QB_BIND_ADDR="127.0.0.1:${API_PORT}" cargo run >/tmp/qb_api_e2e.log 2>&1 &
API_PID=$!

for _ in $(seq 1 60); do
  if curl -fsS "http://127.0.0.1:${API_PORT}/health" >/tmp/qb_e2e_health.json 2>/dev/null; then
    break
  fi
  sleep 1
done
curl -fsS "http://127.0.0.1:${API_PORT}/health" | grep -q '"status":"ok"'

echo "[4/7] Bundle validate + commit"
VALIDATE_RESP=$(curl -fsS -X POST "http://127.0.0.1:${API_PORT}/imports/bundle/validate" \
  -H 'Content-Type: application/json' \
  -d "{\"bundle_path\":\"${BUNDLE_PATH}\",\"allow_similar\":false}")
echo "$VALIDATE_RESP" | grep -q '"ok":true'

COMMIT_RESP=$(curl -fsS -X POST "http://127.0.0.1:${API_PORT}/imports/bundle/commit" \
  -H 'Content-Type: application/json' \
  -d "{\"bundle_path\":\"${BUNDLE_PATH}\",\"allow_similar\":false}")
echo "$COMMIT_RESP" | grep -q '"status":"committed"'
echo "$COMMIT_RESP" | grep -q '"imported_questions":3'

echo "[5/7] Workbook + stats + difficulty"
WB_RESP=$(curl -fsS -X POST "http://127.0.0.1:${API_PORT}/imports/workbooks/commit" \
  -H 'Content-Type: application/json' \
  -d "{\"workbook_path\":\"${WORKBOOK_PATH}\",\"paper_id\":\"${PAPER_ID}\",\"exam_session\":\"demo-session-2025\",\"workbook_kind\":\"paper_registry\",\"workbook_id\":\"${WORKBOOK_ID}\",\"notes\":\"e2e\"}")
echo "$WB_RESP" | grep -q '"workbook_id":"WB-CPHOS-18-DEMO-INDEX"'

STATS_RESP=$(curl -fsS -X POST "http://127.0.0.1:${API_PORT}/imports/stats/commit" \
  -H 'Content-Type: application/json' \
  -d "{\"csv_path\":\"${STATS_CSV}\",\"stats_source\":\"sample_scores\",\"stats_version\":\"demo-v1\",\"source_workbook_id\":\"${WORKBOOK_ID}\"}")
echo "$STATS_RESP" | grep -q '"imported_stats":3'

DIFF_RESP=$(curl -fsS -X POST "http://127.0.0.1:${API_PORT}/difficulty-scores/run" \
  -H 'Content-Type: application/json' \
  -d '{"method_version":"demo-baseline"}')
echo "$DIFF_RESP" | grep -q '"updated_count":3'

echo "[6/7] Query + download"
curl -fsS "http://127.0.0.1:${API_PORT}/papers" | grep -q '"paper_id":"CPHOS-18-REGULAR-DEMO"'
curl -fsS "http://127.0.0.1:${API_PORT}/questions?paper_id=${PAPER_ID}" | grep -q '"question_id":"QB-2024-T-01"'
curl -fsS "http://127.0.0.1:${API_PORT}/search?q=pendulum" | grep -q '"QB-2024-E-02"'

curl -fsS "http://127.0.0.1:${API_PORT}/score-workbooks/${WORKBOOK_ID}/download" -o /tmp/qb_e2e_download.xlsx
[[ -s /tmp/qb_e2e_download.xlsx ]]

echo "[7/7] Export + quality"
EXPORT_JSONL=$(curl -fsS -X POST "http://127.0.0.1:${API_PORT}/exports/run" \
  -H 'Content-Type: application/json' \
  -d '{"format":"jsonl","public":false,"output_path":"/tmp/qb_e2e_internal.jsonl"}')
echo "$EXPORT_JSONL" | grep -q '"exported_questions":3'

EXPORT_CSV=$(curl -fsS -X POST "http://127.0.0.1:${API_PORT}/exports/run" \
  -H 'Content-Type: application/json' \
  -d '{"format":"csv","public":true,"output_path":"/tmp/qb_e2e_public.csv"}')
echo "$EXPORT_CSV" | grep -q '"exported_questions":3'

QUALITY=$(curl -fsS -X POST "http://127.0.0.1:${API_PORT}/quality-checks/run" \
  -H 'Content-Type: application/json' \
  -d '{"output_path":"/tmp/qb_e2e_quality.json"}')
echo "$QUALITY" | grep -q '"missing_workbook_blob":\[\]'

echo "E2E full flow passed."
