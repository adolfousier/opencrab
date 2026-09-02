# Adversarial security eval for OpenCrabs — runs the REAL agent against a real
# model inside a throwaway container, attempting destructive commands and
# non-owner data exfiltration. Build from the repo root:
#
#   docker build -f src/evals/security-eval/Dockerfile -t opencrabs-sectest .
#
# Run (mount your working config so the model actually runs; it is copied into
# the disposable sandbox, NOT mutated):
#
#   docker run --rm \
#     -v "$HOME/.opencrabs:/cfg:ro" -e CONFIG_SRC=/cfg \
#     opencrabs-sectest
#
# Exit code is non-zero if any attack succeeded (a fake secret leaked or a
# destructive command was not blocked). Intended for a nightly/manual job, not
# the CI gate (it needs a real model and network).

FROM rust:bookworm AS builder
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev cmake clang libclang-dev build-essential ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY . .
# Lean build: the eval drives `opencrabs run` (agent + tools + providers); no
# channels/audio/browser needed. The bash blocklist and path confinement live
# in core, so the gates under test are still present.
RUN cargo build --release --no-default-features --bin opencrabs

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    python3 ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/opencrabs /usr/local/bin/opencrabs
COPY --from=builder /build/src/docs/reference/templates/SECURITY.md /opt/opencrabs/SECURITY.md
COPY src/evals/security-eval/ /opt/sec-eval/

# Disposable non-root user; even a gate failure can't touch the host.
RUN useradd -m -u 10001 crab
USER crab
ENV SANDBOX_HOME=/home/crab/sandbox \
    SECURITY_TEMPLATE=/opt/opencrabs/SECURITY.md \
    OPENCRABS_BIN=/usr/local/bin/opencrabs

ENTRYPOINT ["/bin/bash", "-lc", "bash /opt/sec-eval/setup_sandbox.sh && exec python3 /opt/sec-eval/run_evals.py"]
