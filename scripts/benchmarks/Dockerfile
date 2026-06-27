# Multi-stage Dockerfile to build and package hayate for benchmarking
FROM docker.io/library/rust:1.96-alpine AS builder
RUN apk add --no-cache musl-dev gcc make

WORKDIR /src
COPY . .
# Compile the library and CLI in release mode
RUN cargo build --release --workspace

FROM docker.io/library/alpine:latest
RUN apk add --no-cache iperf3 strace curl ca-certificates

# Copy both the CLI binary and the test files if needed
COPY --from=builder /src/target/release/hayate /usr/local/bin/hayate
WORKDIR /workspace
ENTRYPOINT ["/usr/local/bin/hayate"]
