FROM rust:1.98-alpine3.22 AS build
RUN apk add --no-cache musl-dev
WORKDIR /usr/src/harness
COPY Cargo.toml Cargo.lock build.rs ./
COPY src ./src
RUN cargo build --release --locked

FROM alpine:3.24
COPY --from=build /usr/src/harness/target/release/bowtie-rust-corvus-json-schema /usr/local/bin/
CMD ["bowtie-rust-corvus-json-schema"]
