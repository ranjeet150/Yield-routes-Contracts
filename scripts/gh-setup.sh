#!/usr/bin/env bash
# gh-setup.sh — Create labels and open pre-drafted issues for yield-routes-contracts
set -euo pipefail

REPO="Yield-routes/Yield-routes-Contracts"

echo "==> Creating labels in $REPO..."
gh label create "good first issue"  --color "7057ff" --description "Good for newcomers — no deep Soroban knowledge required"             --repo "$REPO" --force
gh label create "help wanted"       --color "008672" --description "Extra attention is needed"                                      --repo "$REPO" --force
gh label create "documentation"     --color "0075ca" --description "Improvements or additions to documentation"                     --repo "$REPO" --force
gh label create "bug"               --color "d73a4a" --description "Something isn't working"                                       --repo "$REPO" --force
gh label create "enhancement"       --color "a2eeef" --description "New feature or improvement to existing functionality"            --repo "$REPO" --force
gh label create "smart-contract"    --color "b60205" --description "Soroban Rust smart contract work"                              --repo "$REPO" --force
gh label create "testing"           --color "f9d0c4" --description "Adding or improving tests"                                     --repo "$REPO" --force
gh label create "security"          --color "e11d48" --description "Security-related issue or improvement"                         --repo "$REPO" --force
echo "    Labels created."

echo ""
echo "==> Opening issues..."

ISSUES_DIR=".github/ISSUES"

# File -> "title|label1,label2,..."
declare -A ISSUE_MAP
ISSUE_MAP["real-cross-contract-pool-calls-in-execute_route.md"]="Real cross-contract pool calls in execute_route|smart-contract,enhancement,help wanted"
ISSUE_MAP["split-route-execution-across-two-pools.md"]="Split route execution across two pools|smart-contract,enhancement,help wanted"
ISSUE_MAP["twap-check-in-execute_route-for-manipulation-protection.md"]="TWAP check in execute_route for manipulation protection|smart-contract,enhancement,security,help wanted"

CREATED=0
for file in "${!ISSUE_MAP[@]}"; do
  IFS='|' read -r title labels <<< "${ISSUE_MAP[$file]}"
  body_file="$ISSUES_DIR/$file"
  if [[ ! -f "$body_file" ]]; then
    echo "    SKIP: $body_file not found"
    continue
  fi
  echo "    Creating: $title"
  gh issue create \
    --repo "$REPO" \
    --title "$title" \
    --body-file "$body_file" \
    --label "$labels"
  sleep 1
  ((CREATED++))
done

echo ""
echo "==> Summary: created $CREATED issue(s) in $REPO"
