FROM oven/bun:1.4.4@sha256:1d22548f177f4ff38e0c15baa3c2c46c6c2060ba2d1bc573bb4720b407548f1d AS builder

WORKDIR /build/web
COPY web/package.json web/bun.lock ./
RUN bun install --frozen-lockfile
COPY ./web ./
COPY ./VERSION /build/VERSION
RUN DISABLE_ESLINT_PLUGIN='true' VITE_REACT_APP_VERSION=$(cat /build/VERSION) bun run build

FROM oven/bun:1@sha256:0733e50325078969732ebe3b15ce4c4be5082f18c4ac1a0f0ca4839c2e4e42a7 AS builder-classic

WORKDIR /build/web/classic
COPY web/classic/package.json web/classic/bun.lock ./
RUN bun install --frozen-lockfile
COPY web/classic ./
COPY ./VERSION /build/VERSION
RUN VITE_REACT_APP_VERSION=$(cat /build/VERSION) bun run build

# The bundled services under services/ are built here so the image ships the
# gateway and every service it publishes.
FROM rust:1-bookworm AS builder-services

WORKDIR /build
ENV CARGO_TARGET_DIR=/build/target

COPY services/ai-bridge/Cargo.toml services/ai-bridge/Cargo.lock ./services/ai-bridge/
COPY services/invitations/Cargo.toml services/invitations/Cargo.lock ./services/invitations/
COPY services/connect/Cargo.toml services/connect/Cargo.lock ./services/connect/
COPY services ./services
RUN cargo build --release --locked --manifest-path services/ai-bridge/Cargo.toml \
    && cargo build --release --locked --manifest-path services/invitations/Cargo.toml \
    && cargo build --release --locked --manifest-path services/connect/Cargo.toml
FROM golang:1.26.10-alpine@sha256:9ce231b28998b32e116927c61aa5c406183061d4f3f2d61b292c88e7f6bdf7ca AS builder2
ENV GO111MODULE=on CGO_ENABLED=0 GOWORK=off

ARG TARGETOS
ARG TARGETARCH
ENV GOOS=${TARGETOS:-linux} GOARCH=${TARGETARCH:-amd64}
ENV GOEXPERIMENT=greenteagc

WORKDIR /build

ADD go.mod go.sum ./
# relaykit and tokenkit are local submodules referenced via replace; their
# go.mod files must be present for go mod download to resolve the main module graph.
ADD relaykit/go.mod ./relaykit/go.mod
ADD tokenkit/go.mod ./tokenkit/go.mod
RUN go mod download

COPY . .
COPY --from=builder /build/web/dist ./web/dist
COPY --from=builder-classic /build/web/classic/dist ./web/classic/dist
RUN go build -pgo="$(go list -m -f '{{.Dir}}' github.com/Calcium-Ion/moejs)/default.pgo" -ldflags "-s -w -X 'github.com/QuantumNous/new-api/common.Version=$(cat VERSION)'" -o new-api

FROM debian:bookworm-slim@sha256:f06537653ac770703bc45b4b113475bd402f451e85223f0f2837acbf89ab020a

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tzdata libasan8 wget \
    && rm -rf /var/lib/apt/lists/* \
    && update-ca-certificates

COPY --from=builder2 /build/new-api /
COPY --from=builder-services /build/target/release/ai-bridge /build/target/release/invitations /build/target/release/connect /usr/local/bin/
COPY services/ai-bridge/cyber_policy_rules.json /usr/local/share/ai-bridge/cyber_policy_rules.json
COPY docker/entrypoint.sh /entrypoint.sh
COPY LICENSE NOTICE THIRD-PARTY-LICENSES.md /licenses/
RUN chmod +x /entrypoint.sh

# Bundled services run next to the gateway. The entrypoint derives ai-bridge's
# upstream from PORT, so nothing has to be restated here.
ENV AI_BRIDGE_CYBER_POLICY_CONFIRMATION_RULES_FILE=/usr/local/share/ai-bridge/cyber_policy_rules.json

EXPOSE 3000
WORKDIR /data
ENTRYPOINT ["/entrypoint.sh"]
