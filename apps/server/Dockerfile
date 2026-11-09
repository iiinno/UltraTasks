# syntax=docker/dockerfile:1
FROM rust:1.99.1-slim-bookworm AS build
WORKDIR /src
COPY . .
# The optional CA is a BuildKit secret: it is used only for this invocation and
# never becomes an image layer. Normal hosts use the builder's public CA roots.
# Registry/target caches avoid copying build output into intermediate layers.
RUN --mount=type=secret,id=proxy_ca \
    --mount=type=cache,id=ultratasks-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=ultratasks-server-target,target=/src/target \
    if [ -f /run/secrets/proxy_ca ]; then export CARGO_HTTP_CAINFO=/run/secrets/proxy_ca; fi; \
    cargo +1.99.1 build --locked --release -p ultratasks-server && \
    mkdir /out && cp target/release/ultratasks-server /out/ultratasks-server

FROM debian:bookworm-slim
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
