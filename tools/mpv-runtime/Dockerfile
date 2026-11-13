FROM ubuntu:24.04@sha256:2198e4c65076e9cb0b3d695219df667c0e3b0072da64ca3c2648aca17dcecaf5
ARG DEBIAN_FRONTEND=noninteractive
# Docker downloads over HTTPS using the host's trust store. Verify the exact
# bootstrap package before extracting its public CA certificates; the minimal
# base image cannot yet verify snapshot HTTPS redirects. Plain ADD plus sha256sum
# also works with the legacy Docker builder (ADD --checksum requires BuildKit).
ADD https://snapshot.ubuntu.com/ubuntu/20261101T000000Z/pool/main/c/ca-certificates/ca-certificates_20240203_all.deb /tmp/ca-certificates.deb
RUN printf '641de77d8f142cfd62a1a6f964ba67b20754d3337c480efb529d086075a06c9a  /tmp/ca-certificates.deb\n' | sha256sum --check - && \
    dpkg-deb --extract /tmp/ca-certificates.deb /tmp/ca-bootstrap && \
    mkdir -p /etc/ssl/certs && \
    cat /tmp/ca-bootstrap/usr/share/ca-certificates/mozilla/*.crt > /etc/ssl/certs/ca-certificates.crt && \
    rm -rf /tmp/ca-bootstrap /tmp/ca-certificates.deb && \
    rm /etc/apt/sources.list.d/ubuntu.sources && \
    printf 'deb https://snapshot.ubuntu.com/ubuntu/20261101T000000Z noble main universe\ndeb https://snapshot.ubuntu.com/ubuntu/20261101T000000Z noble-updates main universe\ndeb https://snapshot.ubuntu.com/ubuntu/20261101T000000Z noble-security main universe\n' > /etc/apt/sources.list && \
    apt-get update && \
    apt-get install -y --no-install-recommends \
      ca-certificates python3 meson ninja-build cmake make gcc g++ pkg-config \
      nasm git patch xz-utils && \
    rm -rf /var/lib/apt/lists/*
WORKDIR /build
