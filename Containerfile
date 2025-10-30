ARG RUST_VERSION=1.90
ARG ALPINE_VERSION=3.22

# Build stage
FROM --platform=$BUILDPLATFORM rust:${RUST_VERSION}-alpine${ALPINE_VERSION} AS builder
WORKDIR /build

# Let Docker/Podman tell us the target we're building for (amd64/arm64)
ARG TARGETPLATFORM
ARG TARGETARCH
# (Not strictly needed with Alpine+musl, but helpful if you ever use non-multiarch bases)

RUN apk add --no-cache gcc musl-dev

# Cache deps
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs
RUN cargo fetch

# Build your real code
COPY . .
# If your binary name differs, change 'codebattle_runner' below.
RUN cargo build --release && strip target/release/codebattle_runner || true

# Runtime stage
FROM alpine:${ALPINE_VERSION}
WORKDIR /app
# Optional: non-root user
RUN adduser -S -u 10001 app
COPY --from=builder /build/target/release/codebattle_runner /app/
USER app

EXPOSE 8000
ENTRYPOINT ["/app/codebattle_runner"]
