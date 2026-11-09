# syntax=docker/dockerfile:1
FROM rust:1.99.1-slim-bookworm@sha256:f2c004e3d2d8b547e8cf7ffbc29205385f4e71d23e53e4e383065db5e9f822cd AS build
# The optional CA is a BuildKit secret: it is used only for this invocation and
# never becomes an image layer. Normal hosts use the builder's public CA roots.
# Registry/target caches avoid copying build output into intermediate layers.
RUN --mount=type=secret,id=proxy_ca \
    --mount=type=bind,source=.,target=/src,readonly \
    --mount=type=cache,id=ultratasks-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=ultratasks-server-target,target=/build-target \
    if [ -f /run/secrets/proxy_ca ]; then export CARGO_HTTP_CAINFO=/run/secrets/proxy_ca; fi; \
    cd /src && CARGO_TARGET_DIR=/build-target cargo +1.99.1 build --locked --release -p ultratasks-server && \
    mkdir /out && cp /build-target/release/ultratasks-server /out/ultratasks-server

FROM debian:bookworm-slim@sha256:734a72b3d6142a71993b4bc580ca01d8ca687f77a2c422d96a961d3376b179bc
RUN groupadd --gid 10001 ultratasks && useradd --uid 10001 --gid 10001 --no-create-home ultratasks && mkdir /data && chown ultratasks:ultratasks /data
# The runtime retains public trust roots for the optional HTTPS sync client CLI.
COPY --from=build /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=build /out/ultratasks-server /usr/local/bin/ultratasks-server
USER 10001:10001
WORKDIR /data
VOLUME /data
EXPOSE 8374
ENTRYPOINT ["ultratasks-server", "--database", "/data/server.db", "--credentials", "/data/devices.db"]
CMD ["serve", "--listen", "0.0.0.0:8374", "--behind-tls-proxy"]
