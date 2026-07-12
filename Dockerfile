FROM rust:1.88 as builder

WORKDIR /usr/src/app
COPY . .
RUN cargo build --release --bin kora

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /usr/src/app/target/release/kora /usr/local/bin/
COPY --from=builder /usr/src/app/kora.toml ./
COPY --from=builder /usr/src/app/signers.toml ./

EXPOSE 8081
CMD ["kora", "--config", "/app/kora.toml", "rpc", "start", "--signers-config", "/app/signers.toml"]