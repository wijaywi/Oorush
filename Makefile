.PHONY: test

test:
	CARGO_NET_OFFLINE=true cargo test --locked
