#!/usr/bin/env bash
set -euo pipefail

CONTAINER_NAME="${CONTAINER_NAME:-qb-postgres-e2e}"
POSTGRES_IMAGE="${POSTGRES_IMAGE:-postgres:14.1}"
POSTGRES_PORT="${POSTGRES_PORT:-55433}"
API_PORT="${API_PORT:-18080}"
DB_URL="postgres://postgres:postgres@127.0.0.1:${POSTGRES_PORT}/qb"
SAMPLE_ROOT="$(pwd)/samples/generated"
PAPER_ID="CPHOS-GEN-REGULAR-DEMO"

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

echo "[1/7] Generate samples from CPHOS-Latex"
bash scripts/generate_samples.sh "$SAMPLE_ROOT" >/tmp/qb_generate_samples.log

echo "[2/7] Start PostgreSQL container"
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

echo "[3/7] Apply migration"
docker exec -i "$CONTAINER_NAME" psql -U postgres -d qb < migrations/0001_init_pg.sql >/dev/null

echo "[4/7] Start API"
QB_DATABASE_URL="$DB_URL" QB_BIND_ADDR="127.0.0.1:${API_PORT}" cargo run >/tmp/qb_api_e2e.log 2>&1 &
API_PID=$!

for _ in $(seq 1 60); do
  if curl -fsS "http://127.0.0.1:${API_PORT}/health" >/tmp/qb_e2e_health.json 2>/dev/null; then
    break
  fi
  sleep 1
done
curl -fsS "http://127.0.0.1:${API_PORT}/health" | grep -q '"status":"ok"'

echo "[5/7] Import questions"
VALIDATE_RESP=$(curl -fsS -X POST "http://127.0.0.1:${API_PORT}/questions/imports/validate" \
  -H 'Content-Type: application/json' \
  -d "{\"source_root\":\"${SAMPLE_ROOT}\",\"allow_similar\":false}")
echo "$VALIDATE_RESP" | grep -q '"ok":true'

IMPORT_RESP=$(curl -fsS -X POST "http://127.0.0.1:${API_PORT}/questions/imports/commit" \
  -H 'Content-Type: application/json' \
  -d "{\"source_root\":\"${SAMPLE_ROOT}\",\"allow_similar\":false}")
echo "$IMPORT_RESP" | grep -q '"status":"committed"'
echo "$IMPORT_RESP" | grep -q '"imported_questions":4'

echo "[6/7] Create paper and bind question order"
CREATE_PAPER=$(curl -fsS -X POST "http://127.0.0.1:${API_PORT}/papers" \
  -H 'Content-Type: application/json' \
  --data-binary "@${SAMPLE_ROOT}/api/create_paper.json")
echo "$CREATE_PAPER" | grep -q "\"paper_id\":\"${PAPER_ID}\""

REPLACE_REFS=$(curl -fsS -X PUT "http://127.0.0.1:${API_PORT}/papers/${PAPER_ID}/questions" \
  -H 'Content-Type: application/json' \
  --data-binary "@${SAMPLE_ROOT}/api/replace_paper_questions.json")
echo "$REPLACE_REFS" | grep -q '"question_count":4'

echo "[7/7] Query + export + quality"
curl -fsS "http://127.0.0.1:${API_PORT}/papers" | grep -q "\"paper_id\":\"${PAPER_ID}\""
curl -fsS "http://127.0.0.1:${API_PORT}/papers/${PAPER_ID}" | grep -q '"question_label":"1"'
curl -fsS "http://127.0.0.1:${API_PORT}/questions?paper_id=${PAPER_ID}" | grep -q '"question_id":"GEN-T-01"'
curl -fsS "http://127.0.0.1:${API_PORT}/questions/GEN-E-01" | grep -q "\"paper_id\":\"${PAPER_ID}\""
curl -fsS "http://127.0.0.1:${API_PORT}/search?q=thermal" | grep -q '"GEN-E-02'

EXPORT_JSONL=$(curl -fsS -X POST "http://127.0.0.1:${API_PORT}/exports/run" \
  -H 'Content-Type: application/json' \
  -d '{"format":"jsonl","public":false,"output_path":"/tmp/qb_e2e_internal.jsonl"}')
echo "$EXPORT_JSONL" | grep -q '"exported_questions":4'

QUALITY=$(curl -fsS -X POST "http://127.0.0.1:${API_PORT}/quality-checks/run" \
  -H 'Content-Type: application/json' \
  -d '{"output_path":"/tmp/qb_e2e_quality.json"}')
echo "$QUALITY" | grep -q '"empty_papers":\[\]'

echo "E2E full flow passed."
