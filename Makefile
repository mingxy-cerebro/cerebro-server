.PHONY: build test clippy fmt docker run web

build:
	cargo build --release

test:
	cargo test

clippy:
	cargo clippy -- -D warnings

fmt:
	cargo fmt -- --check

docker:
	docker build -t omem-server .

run:
	cargo run --release

web:
	cd omem-web && npm run build
	rm -rf plugins/opencode/web plugins/claude-code/web
	cp -r omem-web/dist plugins/opencode/web
	cp -r omem-web/dist plugins/claude-code/web
