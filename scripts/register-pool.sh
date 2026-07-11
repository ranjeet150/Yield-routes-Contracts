#!/usr/bin/env bash
# Register an AMM pool with the RouteAggregator
set -euo pipefail
source .env.deployed
NETWORK="${1:-testnet}"
TOKEN_A="${2:?Usage: register-pool.sh [network] [token_a] [token_b] [pool_address]}"
TOKEN_B="${3:?}"
POOL="${4:?}"

echo "🔀  Registering pool $POOL ($TOKEN_A / $TOKEN_B) on $NETWORK..."
stellar contract invoke --id "$ROUTE_AGGREGATOR_ID" --source deployer --network "$NETWORK" \
  -- register_pool --token_a "$TOKEN_A" --token_b "$TOKEN_B" --pool "$POOL"
echo "✅  Pool registered."
