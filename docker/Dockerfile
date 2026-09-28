# ============================================================================
# Freebuff2API Rust 版 — 多阶段构建（Railway / 通用容器）
#
# 与原版 Dockerfile 的差异（都为了 PaaS 上能一把跑起来）：
#   1. ENTRYPOINT 换成 docker/entrypoint.sh —— 原版直接 `--config /data/config.json`，
#      容器里没有这个文件时进程会立刻退出 → 平台判定部署失败 → crash-loop。
#   2. HEALTHCHECK 用 $PORT 而不是写死 47821 —— 否则平台改了端口健康检查恒失败。
#   3. 默认监听 0.0.0.0 —— 容器里绑 127.0.0.1 等于对外不可达。
#   4. 不声明 VOLUME —— 由平台挂载（Railway 的卷挂到 /data）；
#      声明 VOLUME 会让平台卷与镜像匿名卷打架，数据落哪儿说不清。
#   5. cargo 加 --locked —— Cargo.lock 已入库，锁死依赖版本，避免构建漂移。
# ============================================================================

ARG RUST_IMAGE=rust:1.95-bookworm
ARG RUNTIME_IMAGE=debian:bookworm-slim

# ---------------------------------------------------------------- 构建阶段
FROM ${RUST_IMAGE} AS builder

ENV CARGO_NET_RETRY=5 \
    CARGO_TERM_COLOR=never

WORKDIR /build

# 先只拷清单 + 造一个空壳 main/lib，把「依赖编译」单独固化成一层。
# 之后只改业务代码时这一层直接命中缓存，构建时间从十几分钟掉到一分钟级。
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src \
 && echo 'fn main(){}' > src/main.rs \
 && echo '' > src/lib.rs \
 && cargo build --release --locked 2>/dev/null || true

COPY src/ src/
RUN cargo build --release --locked

# ---------------------------------------------------------------- 运行阶段
FROM ${RUNTIME_IMAGE}

# openssl 给 entrypoint 生成 api key，curl 给 HEALTHCHECK，ca-certificates 给上游 TLS。
# 二进制的 TLS 走 rustls（纯 Rust），所以运行时不依赖 libssl。
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl openssl \
 && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/freebuff2api /usr/local/bin/freebuff2api

# entrypoint 在 Windows 上编辑后可能带 CRLF，CR 会让 /bin/sh 报 "not found"。
# 这里顺手归一化，避免在 Windows 上改完脚本就炸。
COPY docker/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN sed -i 's/\r$//' /usr/local/bin/entrypoint.sh \
 && chmod +x /usr/local/bin/entrypoint.sh

# PORT 由平台注入（Railway/Render/Fly 都叫 PORT），src/config.rs 的 PORT 处理会接管；
# 不带 PORT 时回落到 47821。LISTEN_ADDR 在这里给容器默认值（0.0.0.0），
# 用户仍可用 LISTEN_ADDR 覆盖 host。
ENV PORT=47821 \
    DATA_DIR=/data \
    LISTEN_ADDR=0.0.0.0:47821 \
    RUST_LOG=info

EXPOSE 47821

# shell 形式（不是 exec 形式）才能展开 ${PORT}
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
  CMD curl -sf "http://127.0.0.1:${PORT}/healthz" >/dev/null || exit 1

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
