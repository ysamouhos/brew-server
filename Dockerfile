FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock build.rs ./
COPY third_party ./third_party
COPY src ./src
COPY assets ./assets
RUN cargo build --release --bins

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 --create-home --home-dir /var/lib/brew-server brew \
    && apt-get update && apt-get install -y --no-install-recommends curl ca-certificates \
       iproute2 iputils-arping iputils-ping libcap2-bin \
    && rm -rf /var/lib/apt/lists/* \
    # [ha]: the non-root brew user moves the VIP and pings through these, using
    # the NET_ADMIN/NET_RAW the container is given with cap_add (without
    # cap_add, `ip` would refuse to start, but it only runs with [ha] enabled).
    # readlink -f: setcap refuses symlinks, and the `ip` found first in PATH,
    # /usr/sbin/ip, is a symlink to /bin/ip.
    && setcap cap_net_admin+ep "$(readlink -f "$(command -v ip)")" \
    && setcap cap_net_raw+ep "$(readlink -f "$(command -v arping)")" \
    && setcap cap_net_raw+ep "$(readlink -f "$(command -v ping)")"
COPY --from=build /src/target/release/brew-server /usr/local/bin/brew-server
COPY --from=build /src/target/release/brew-history /usr/local/bin/brew-history
COPY sample/brew-server.toml /etc/brew-server.toml
WORKDIR /var/lib/brew-server
RUN chown brew:brew /var/lib/brew-server
USER brew
# Brew (9000), Basestation Telemetry (9001), Basestation Control (9002),
# monitoring dashboard (9003), SIP signalling (5060/udp) -- all off by
# default except Brew/dashboard, see sample/brew-server.toml. The RTP media port
# range (default 16000-17000/udp, [sip] rtp_port_min/max) is not fixed-size
# enough to EXPOSE meaningfully; publish it explicitly in the compose file
# or `docker run -p` instead.
EXPOSE 9000 9001 9002 9003 5060/udp
# Tries both schemes since [tls] may be enabled or not depending on
# brew-server.toml (the shipped example has it on); -k skips cert
# verification, fine for a same-host healthcheck.
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s \
    CMD curl -fsSk https://localhost:9000/healthz || curl -fsS http://localhost:9000/healthz || exit 1
ENTRYPOINT ["/usr/local/bin/brew-server", "/etc/brew-server.toml"]
