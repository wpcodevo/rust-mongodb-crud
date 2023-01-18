dev:
	docker-compose up -d

dev-down:
	docker-compose down

start-server:
	cargo watch -q -c -w src/ -x run

install:
	cargo add warp
	cargo add mongodb --features bson-chrono-0_4
	cargo add futures --features async-await --no-default-features
	cargo add serde --features derive
	cargo add thiserror
	cargo add chrono --features serde
	cargo add tokio --features full
	cargo add dotenv
	cargo add pretty_env_logger
	# HotReload
	cargo install cargo-watch 