FROM rust:1.76.0-alpine3.18 AS builder

WORKDIR /build

RUN apk add --no-cache gcc libc-dev
COPY Cargo.toml Cargo.lock ./

RUN mkdir src && echo 'fn main() {}' > src/main.rs
RUN cargo fetch
RUN cargo build --release

COPY src src/
RUN touch src/main.rs && cargo build --release

FROM alpine:3.18

WORKDIR /app

COPY --from=builder /build/target/release/codebattle_runner ./

EXPOSE 8000

ENTRYPOINT ["/app/codebattle_runner"]
