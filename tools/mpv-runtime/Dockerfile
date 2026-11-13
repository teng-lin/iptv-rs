FROM ubuntu:24.04@sha256:2198e4c65076e9cb0b3d695219df667c0e3b0072da64ca3c2648aca17dcecaf5
ARG DEBIAN_FRONTEND=noninteractive
# Fixed host package snapshot; compilation itself runs with --network=none.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates=20240203 openssl=3.0.13-0ubuntu3.16 && \
    rm /etc/apt/sources.list.d/ubuntu.sources && \
    printf 'deb http://snapshot.ubuntu.com/ubuntu/20261101T000000Z noble main universe\ndeb http://snapshot.ubuntu.com/ubuntu/20261101T000000Z noble-updates main universe\ndeb http://snapshot.ubuntu.com/ubuntu/20261101T000000Z noble-security main universe\n' > /etc/apt/sources.list && \
    apt-get update && \
    apt-get install -y --no-install-recommends \
      ca-certificates python3 meson ninja-build cmake make gcc g++ pkg-config \
      nasm git patch xz-utils && \
    rm -rf /var/lib/apt/lists/*
WORKDIR /build
