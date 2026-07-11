.PHONY: build test fmt lint deploy-testnet deploy-mainnet clean

build:
	cargo build --release --target wasm32-unknown-unknown

test:
	cargo test -- --nocapture

fmt:
	cargo fmt

lint:
	cargo clippy -- -D warnings

deploy-testnet:
	./scripts/deploy.sh testnet
	./scripts/init-contracts.sh testnet

deploy-mainnet:
	./scripts/deploy.sh mainnet
	./scripts/init-contracts.sh mainnet

clean:
	cargo clean
