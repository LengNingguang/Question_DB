#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LATEX_ROOT="${ROOT_DIR}/CPHOS-Latex"
OUT_DIR="${1:-${ROOT_DIR}/samples/generated}"

require_file() {
  if [[ ! -f "$1" ]]; then
    echo "missing required file: $1" >&2
    exit 1
  fi
}

require_file "${LATEX_ROOT}/theory/examples/example-problem.tex"
require_file "${LATEX_ROOT}/experiment/examples/example-paper.tex"
require_file "${LATEX_ROOT}/theory/assets/fig1.jpg"
require_file "${LATEX_ROOT}/experiment/assets/fig1.jpg"

rm -rf "${OUT_DIR}"
mkdir -p \
  "${OUT_DIR}/questions" \
  "${OUT_DIR}/latex/questions" \
  "${OUT_DIR}/assets" \
  "${OUT_DIR}/api"

cp "${LATEX_ROOT}/theory/examples/example-problem.tex" "${OUT_DIR}/latex/questions/GEN-T-01.tex"
cp "${LATEX_ROOT}/theory/examples/example-problem.tex" "${OUT_DIR}/latex/questions/GEN-T-02.tex"
cp "${LATEX_ROOT}/experiment/examples/example-paper.tex" "${OUT_DIR}/latex/questions/GEN-E-01.tex"
cp "${LATEX_ROOT}/experiment/examples/example-paper.tex" "${OUT_DIR}/latex/questions/GEN-E-02.tex"

cp "${LATEX_ROOT}/theory/assets/fig1.jpg" "${OUT_DIR}/assets/GEN-T-02-fig1.jpg"
cp "${LATEX_ROOT}/experiment/assets/fig1.jpg" "${OUT_DIR}/assets/GEN-E-01-fig1.jpg"
cp "${LATEX_ROOT}/experiment/assets/fig1.jpg" "${OUT_DIR}/assets/GEN-E-02-fig1.jpg"

SHA_T02="$(sha256sum "${OUT_DIR}/assets/GEN-T-02-fig1.jpg" | awk '{print toupper($1)}')"
SHA_E01="$(sha256sum "${OUT_DIR}/assets/GEN-E-01-fig1.jpg" | awk '{print toupper($1)}')"
SHA_E02="$(sha256sum "${OUT_DIR}/assets/GEN-E-02-fig1.jpg" | awk '{print toupper($1)}')"

cat > "${OUT_DIR}/manifest.json" <<'EOF'
{
  "source_name": "generated-cphos-latex",
  "run_label": "generated-question-import",
  "notes": "Generated from the local CPHOS-Latex workspace"
}
EOF

cat > "${OUT_DIR}/questions/GEN-T-01.json" <<'EOF'
{
  "question_id": "GEN-T-01",
  "category": "theory",
  "latex_path": "latex/questions/GEN-T-01.tex",
  "search_text": "generated theory mechanics sample one",
  "status": "reviewed",
  "tags": ["generated", "theory", "mechanics"],
  "notes": "Generated from CPHOS-Latex theory example",
  "assets": []
}
EOF

cat > "${OUT_DIR}/questions/GEN-T-02.json" <<EOF
{
  "question_id": "GEN-T-02",
  "category": "theory",
  "latex_path": "latex/questions/GEN-T-02.tex",
  "search_text": "generated theory optics sample two",
  "status": "published",
  "tags": ["generated", "theory", "optics"],
  "notes": "Generated from CPHOS-Latex theory example with one figure",
  "assets": [
    {
      "asset_id": "GEN-T-02-ASSET-01",
      "kind": "figure",
      "file_path": "assets/GEN-T-02-fig1.jpg",
      "sha256": "${SHA_T02}",
      "caption": "Generated theory reference figure",
      "sort_order": 1
    }
  ]
}
EOF

cat > "${OUT_DIR}/questions/GEN-E-01.json" <<EOF
{
  "question_id": "GEN-E-01",
  "category": "experiment",
  "latex_path": "latex/questions/GEN-E-01.tex",
  "search_text": "generated experiment electricity sample one",
  "status": "reviewed",
  "tags": ["generated", "experiment", "electricity"],
  "notes": "Generated from CPHOS-Latex experiment example",
  "assets": [
    {
      "asset_id": "GEN-E-01-ASSET-01",
      "kind": "figure",
      "file_path": "assets/GEN-E-01-fig1.jpg",
      "sha256": "${SHA_E01}",
      "caption": "Generated experiment figure one",
      "sort_order": 1
    }
  ]
}
EOF

cat > "${OUT_DIR}/questions/GEN-E-02.json" <<EOF
{
  "question_id": "GEN-E-02",
  "category": "experiment",
  "latex_path": "latex/questions/GEN-E-02.tex",
  "search_text": "generated experiment thermal sample two",
  "status": "published",
  "tags": ["generated", "experiment", "thermal"],
  "notes": "Generated from CPHOS-Latex experiment example",
  "assets": [
    {
      "asset_id": "GEN-E-02-ASSET-01",
      "kind": "figure",
      "file_path": "assets/GEN-E-02-fig1.jpg",
      "sha256": "${SHA_E02}",
      "caption": "Generated experiment figure two",
      "sort_order": 1
    }
  ]
}
EOF

cat > "${OUT_DIR}/api/create_paper.json" <<'EOF'
{
  "paper_id": "CPHOS-GEN-REGULAR-DEMO",
  "edition": "generated",
  "paper_type": "regular",
  "title": "Generated Demo Paper",
  "notes": "Paper metadata generated from CPHOS-Latex sample inputs"
}
EOF

cat > "${OUT_DIR}/api/replace_paper_questions.json" <<'EOF'
{
  "question_refs": [
    {
      "question_id": "GEN-T-01",
      "sort_order": 1,
      "question_label": "1"
    },
    {
      "question_id": "GEN-T-02",
      "sort_order": 2,
      "question_label": "2"
    },
    {
      "question_id": "GEN-E-01",
      "sort_order": 3,
      "question_label": "3"
    },
    {
      "question_id": "GEN-E-02",
      "sort_order": 4,
      "question_label": "4"
    }
  ]
}
EOF

echo "Generated sample source at ${OUT_DIR}"
