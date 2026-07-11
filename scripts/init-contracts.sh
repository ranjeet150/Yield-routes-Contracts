#!/usr/bin/env bash
set -euo pipefail
source .env.deployed
NETWORK="${1:-testnet}"
ADMIN=$(stellar keys address deployer)
USDC="${USDC_CONTRACT_ID:?Set USDC_CONTRACT_ID in environment}"

echo "🔧  Initializing YieldRoutes contracts on $NETWORK..."
echo "    Admin:  $ADMIN"
echo "    USDC:   $USDC"

# RouteAggregator: admin, fee_collector=admin, protocol_fee=10bps (0.1%)
stellar contract invoke --id "$ROUTE_AGGREGATOR_ID" --source deployer --network "$NETWORK" \
  -- initialize --admin "$ADMIN" --fee_collector "$ADMIN" --protocol_fee_bps 10
echo "    ✓ RouteAggregator initialized (fee: 0.1%)"

# YieldVault (SEP-56): admin, asset=USDC, fee_collector, name, symbol, decimals, performance_fee_bps
stellar contract invoke --id "$YIELD_VAULT_ID" --source deployer --network "$NETWORK" \
  -- initialize \
  --admin "$ADMIN" \
  --asset "$USDC" \
  --fee_collector "$FEE_DISTRIBUTOR_ID" \
  --name "YieldRoutes USDC" \
  --symbol "yrUSDC" \
  --decimals 7 \
  --performance_fee_bps 1000
echo "    ✓ YieldVault initialized (SEP-56, 10% performance fee)"

# FeeDistributor: treasury=admin, yield_vault=VAULT, lp_rewards=admin
stellar contract invoke --id "$FEE_DISTRIBUTOR_ID" --source deployer --network "$NETWORK" \
  -- initialize \
  --admin "$ADMIN" \
  --token_id "$USDC" \
  --treasury "$ADMIN" \
  --yield_vault "$YIELD_VAULT_ID" \
  --lp_rewards "$ADMIN" \
  --treasury_bps 5000 \
  --vault_bps 3000 \
  --lp_bps 2000
echo "    ✓ FeeDistributor initialized (50/30/20 split)"

# PriceOracle: admin, max_deviation=200bps (2%)
stellar contract invoke --id "$PRICE_ORACLE_ID" --source deployer --network "$NETWORK" \
  -- initialize --admin "$ADMIN" --max_deviation_bps 200
echo "    ✓ PriceOracle initialized"

stellar contract invoke --id "$PRICE_ORACLE_ID" --source deployer --network "$NETWORK" \
  -- add_reporter --reporter "$ADMIN"
echo "    ✓ Admin added as price reporter"

echo ""
echo "✅  YieldRoutes fully initialized."
echo "    YieldVault is SEP-56 compliant — query_asset() returns: $USDC"
