#!/usr/bin/env bash
set -euo pipefail

NETWORK="${1:-testnet}"
echo "🚀  Deploying YieldRoutes contracts to Stellar $NETWORK..."

if [ "$NETWORK" = "testnet" ]; then
  NET_ARGS="--network testnet"
  RPC_URL="https://soroban-testnet.stellar.org"
  PASSPHRASE="Test SDF Network ; September 2015"
else
  NET_ARGS="--network mainnet"
  RPC_URL="https://soroban-mainnet.stellar.org"
  PASSPHRASE="Public Global Stellar Network ; September 2015"
fi

DEPLOYER="${STELLAR_DEPLOYER_KEY:-deployer}"

echo "📦  Building all contracts..."
cargo build --release --target wasm32-unknown-unknown

echo "🔀  Deploying RouteAggregator..."
ROUTE_ID=$(stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/route_aggregator.wasm \
  --source "$DEPLOYER" $NET_ARGS 2>&1 | tail -1)
echo "    RouteAggregator: $ROUTE_ID"

echo "🌱  Deploying YieldVault..."
VAULT_ID=$(stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/yield_vault.wasm \
  --source "$DEPLOYER" $NET_ARGS 2>&1 | tail -1)
echo "    YieldVault:      $VAULT_ID"

echo "💰  Deploying FeeDistributor..."
FEE_ID=$(stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/fee_distributor.wasm \
  --source "$DEPLOYER" $NET_ARGS 2>&1 | tail -1)
echo "    FeeDistributor:  $FEE_ID"

echo "📊  Deploying PriceOracle..."
ORACLE_ID=$(stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/price_oracle.wasm \
  --source "$DEPLOYER" $NET_ARGS 2>&1 | tail -1)
echo "    PriceOracle:     $ORACLE_ID"

cat > .env.deployed << EOF
ROUTE_AGGREGATOR_ID=$ROUTE_ID
YIELD_VAULT_ID=$VAULT_ID
FEE_DISTRIBUTOR_ID=$FEE_ID
PRICE_ORACLE_ID=$ORACLE_ID
STELLAR_RPC_URL=$RPC_URL
STELLAR_NETWORK_PASSPHRASE=$PASSPHRASE
EOF

echo ""
echo "✅  All YieldRoutes contracts deployed."
echo "    IDs written to .env.deployed"
echo "    Next: ./scripts/init-contracts.sh $NETWORK"
