FROM rust:alpine AS builder
WORKDIR /usr/home/metrics

RUN apk add --no-cache git openssl-dev pkgconfig openssl-libs-static
RUN git clone https://github.com/rust-lang/crates.io-index.git /usr/share/crates.io-index

RUN git clone --depth 1 https://github.com/denmarkx/rust-metrics .
RUN cargo build --release

FROM alpine:latest
RUN apk add --no-cache ca-certificates
COPY --from=builder /usr/home/metrics/target/release/metrics /usr/home/rust_metrics
COPY --from=builder /usr/share/crates.io-index /usr/share/crates.io-index
ENV CARGO_REGISTRY=/usr/share/crates.io-index/.git

RUN mkdir /usr/home/crates

ENTRYPOINT ["/usr/home/rust_metrics"]
VOLUME ["/usr/home/crates"]