# Task image for the CA-ID index regeneration pipeline: the clingen-index binary
# plus the tools the pipeline scripts and Nextflow's task wrapper call.
#
#   docker build -t clingen-index:0.1.0 .
#
# Base images are pinned by (index) digest. Keep the two debian FROM lines identical.

# --- build: compile clingen-index against the pinned toolchain -------------------
# rust:1.94.1-bookworm already ships the 1.94.1 toolchain that rust-toolchain.toml names.
FROM rust:1.94.1-bookworm@sha256:6ae102bdbf528294bc79ad6e1fae682f6f7c2a6e6621506ba959f9685b308a55 AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
RUN cargo build --release --locked -p clingen-index

# --- awscli: pinned AWS CLI v2 (Nextflow's awsbatch executor copies work files with it)
# unzip and the download tools stay in this stage; only /usr/local/aws-cli is copied on.
# debian:bookworm-20261005-slim = Debian 12.15, pushed 2026-10-06
FROM debian:bookworm-20261005-slim@sha256:7c7b2c966bc9ee8cedfeef67e0e279108992c77681fa595db4a9d65c06ccc587 AS awscli
# BuildKit sets TARGETARCH (amd64 or arm64). The legacy `docker build` (no buildx
# installed) leaves it empty, so fall back to dpkg, which uses the same names.
ARG TARGETARCH
ARG AWSCLI_VERSION=2.37.12
RUN set -eux; \
    apt-get update; \
    DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends ca-certificates curl unzip; \
    arch="${TARGETARCH:-$(dpkg --print-architecture)}"; \
    case "${arch}" in \
      amd64) A=x86_64;  SHA=e6d3c672fb36ed4b12c354c39a993eee2b34da1437cafb10bac4480840cfa83f ;; \
      arm64) A=aarch64; SHA=829c76096e05f743267d75098bc7cc7cfae799fc7e568554ebaa6d78e6aba753 ;; \
      *) echo "unsupported arch ${arch}" >&2; exit 1 ;; \
    esac; \
    curl -fsSL -o /tmp/awscliv2.zip "https://awscli.amazonaws.com/awscli-exe-linux-${A}-${AWSCLI_VERSION}.zip"; \
    echo "${SHA}  /tmp/awscliv2.zip" | sha256sum -c -; \
    unzip -q /tmp/awscliv2.zip -d /tmp; \
    /tmp/aws/install --install-dir /usr/local/aws-cli --bin-dir /usr/local/bin
# The installer lays out /usr/local/aws-cli/v2/<version> and an absolute symlink v2/current.

# --- runtime: what a pipeline task runs in ----------------------------------------
# No ENTRYPOINT and no CMD of our own: Nextflow starts its bash launcher itself.
# procps provides `ps`, which Nextflow's task metrics need. groff and less are left out:
# only `aws help` and the interactive pager use them, and AWS_PAGER="" covers the pager.
FROM debian:bookworm-20261005-slim@sha256:7c7b2c966bc9ee8cedfeef67e0e279108992c77681fa595db4a9d65c06ccc587
LABEL org.opencontainers.image.version=0.1.0
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      bash coreutils gawk gzip curl ca-certificates procps openssl \
 && rm -rf /var/lib/apt/lists/*
ENV AWS_PAGER=""
COPY --from=awscli /usr/local/aws-cli /usr/local/aws-cli
RUN ln -s /usr/local/aws-cli/v2/current/bin/aws /usr/local/bin/aws && aws --version
COPY --from=build /src/target/release/clingen-index /usr/local/bin/clingen-index
