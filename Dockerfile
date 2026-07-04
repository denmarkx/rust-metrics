FROM rust:alpine AS builder
WORKDIR /usr/home/metrics

RUN apk add --no-cache git openssl-dev pkgconfig openssl-libs-static
RUN --mount=type=cache,id=crates-index-cache,target=/tmp/crates-index \
    if [ ! -d /tmp/crates-index/.git ]; then \
        git clone --depth 1 \
        https://github.com/rust-lang/crates.io-index.git \
        /tmp/crates-index; \
    fi && \
    cp -r /tmp/crates-index /usr/share/crates.io-index

COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release

FROM alpine:latest
RUN apk add --no-cache ca-certificates
COPY --from=builder /usr/home/metrics/target/release/metrics /usr/home/rust_metrics
COPY --from=builder /usr/share/crates.io-index /usr/share/crates.io-index
ENV CARGO_REGISTRY=/usr/share/crates.io-index/.git

RUN mkdir /usr/home/crates

WORKDIR /usr/home/
ENTRYPOINT ["/bin/sh"]
VOLUME ["/usr/home/crates"]