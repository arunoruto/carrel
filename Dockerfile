# syntax=docker/dockerfile:1
# `carrel serve` in a container: the library lives in /library, the config in /config/carrel/config.toml,
# the search index in /cache. See docs/src/server.md.

FROM docker.io/library/rust:1-slim-trixie AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src src
# The runtime image's Python runs the extractor, so nothing is downloaded when a PDF comes in.
ENV CARREL_DEFAULT_EXTRACTOR='python -I -c "$CARREL_EXTRACT_PY" "$PDF"'
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked && cp target/release/carrel /carrel

FROM docker.io/library/python:3.13-slim-trixie
RUN pip install --no-cache-dir pymupdf4llm==1.28.2 \
    && useradd --create-home --uid 1000 carrel \
    && mkdir -p /library /config /cache \
    && chown carrel:carrel /library /config /cache
COPY --from=build /carrel /usr/local/bin/carrel
USER carrel
ENV CARREL_HOME=/library XDG_CONFIG_HOME=/config XDG_CACHE_HOME=/cache
VOLUME ["/library", "/cache"]
EXPOSE 8080
HEALTHCHECK CMD ["python", "-c", "import urllib.request; urllib.request.urlopen('http://127.0.0.1:8080/health')"]
ENTRYPOINT ["carrel"]
CMD ["serve", "--listen", "0.0.0.0:8080"]
