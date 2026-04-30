# syntax=docker/dockerfile:1

# ──────────────────────────────────────────────────────────
# Stage 1 — Builder
# ──────────────────────────────────────────────────────────
FROM rust:alpine3.23 AS builder

# Install build dependencies (notmuch headers + git for batdoc-core)
RUN apk add --no-cache \
    notmuch-dev \
    pkgconf \
    git

# Pin to a specific nightly that matches the local dev environment.
# The codebase does not use nightly feature gates, but we pin the nightly
# for reproducibility and to stay aligned with the lockfile.
RUN rustup toolchain install nightly-2026-04-29 && \
    rustup default nightly-2026-04-29

# Alpine's musl target defaults to static linking, but notmuch is only
# available as a shared library (libnotmuch.so). Disable static CRT so
# the binary links dynamically against musl libc and libnotmuch.
ENV RUSTFLAGS="-C target-feature=-crt-static"

WORKDIR /app

# Cache dependencies: copy manifests first and build a stub binary
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs
RUN cargo build --release && rm -rf src

# Copy full source and rebuild with the real code
COPY . .
RUN touch src/main.rs
RUN cargo build --release

# ──────────────────────────────────────────────────────────
# Stage 2 — Runtime
# ──────────────────────────────────────────────────────────
FROM alpine:3.23

# Runtime: only the notmuch shared library + CA certs for HTTPS
RUN apk add --no-cache \
    notmuch-libs \
    ca-certificates

# Create non-root user (UID 1000)
RUN adduser -D -s /bin/sh -u 1000 rummage

WORKDIR /app
COPY --from=builder /app/target/release/rummage /usr/local/bin/rummage

# Mount your maildir here.  The notmuch database is auto-created inside
# it at /mail/.notmuch/ on first run — no config file is required.
VOLUME /mail

# Optional: mount a directory with a notmuch-config file if you need
# custom indexing settings (tags, hooks, etc.).  Most Docker users can
# leave this empty.
VOLUME /notmuch

# Standard notmuch config path.  libnotmuch reads this automatically;
# rummage also falls back to it when RUMMAGE_NOTMUCH_CONFIG is unset.
# If the file does not exist, rummage auto-initialises the database
# inside the maildir and ignores this variable.
ENV NOTMUCH_CONFIG=/notmuch/notmuch-config

USER rummage

EXPOSE 8000

ENTRYPOINT ["/usr/local/bin/rummage"]
CMD ["--maildir", "/mail", "--host", "0.0.0.0"]
