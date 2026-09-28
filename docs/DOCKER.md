# Docker — Freebuff2API 容器化（v0.10）

> 部署到 Railway / 其他 PaaS 请看 **[RAILWAY.md](RAILWAY.md)** —— 本文档只讲通用容器用法。
> 本机无 Docker 时，镜像构建/推送由 CI（`.github/workflows/docker.yml`）在 main 分支 push 时自动完成
> （多架构 amd64+arm64 → `ghcr.io/<仓库 owner>/freebuff2api`）。

## 一、镜像

- 注册表：`ghcr.io/<仓库 owner>/freebuff2api`（owner 取 CI 里的 `github.repository_owner`，不再是固定值）
- 标签：`latest`（默认分支）、`dev`（dev 分支）、sha 标签
- Dockerfile：`docker/Dockerfile`（多阶段：rust:1.95-bookworm 构建 → debian:bookworm-slim 运行；
  运行时依赖 ca-certificates / curl / openssl；TLS 走 rustls 纯 Rust，无需 OpenSSL 库）
- 入口：`docker/entrypoint.sh`（建数据目录 → 补 `API_KEYS` → exec 网关）

## 二、本机实跑（需 Docker）

> 容器默认已监听 `0.0.0.0`，并读取平台注入的 `PORT`（不带则用 47821）；
> 数据路径由 `DATA_DIR`（默认 `/data`）统一接管，所以**不再需要手写 config.json**。

```bash
# 构建
docker build -f docker/Dockerfile -t freebuff2api:local .

# 运行（最小：端口 + 持久卷 + 一个 API Key）
docker run -d --name fba \
  -p 47821:47821 \
  -e API_KEYS=sk-local \
  -e AUTH_TOKENS=你的freebuff凭证 \
  -v fba-data:/data \
  freebuff2api:local

# 不传 API_KEYS 也行：入口会随机生成一个并打印到容器日志
docker logs fba 2>&1 | grep -A2 "API_KEYS"

# 冒烟（注意 /healthz 与 /v1/models 在配置了 api_keys 后需要鉴权）
curl -sf http://127.0.0.1:47821/healthz && echo OK
curl -sf -H "Authorization: Bearer sk-local" http://127.0.0.1:47821/v1/models | head -c 200

# 清理
docker rm -f fba
```

想用自己的 `config.json`：放到卷里（`/data/config.json`），入口脚本存在时会改用 `--config` 启动，
环境变量仍然优先覆盖。注意容器里 `listen_addr` 要写 `0.0.0.0`，路径建议写 `/data/...` 绝对路径。

## 三、已知改进项

- ✅ v0.10.2：Dockerfile 已加 `HEALTHCHECK`（运行阶段安装 curl，`curl -sf /healthz`，interval 30s / start_period 10s）
- ✅ 本次：`HEALTHCHECK` 改用 `$PORT`（原先写死 47821，平台改端口后健康检查恒失败）
- 容器内 `thread_cleanup_interval_sec`/`thread_max_age_hours` 默认沿用 config 默认；
  多实例部署时注意 `/data` 卷唯一，且**不要开多副本**（多个进程会同时写同一个 SQLite）。

## 四、CI 证据（2026-09-19）

- `Build & Push Docker Image`（docker.yml）在 main push（b561752）→ **success**（amd64 + arm64 双平台构建 + GHCR manifest 合并推送）
- 本机环境限制：原开发机无 docker CLI，容器内 healthz 实跑需在有 Docker 的主机上按第二节命令执行（命令即验证脚本）。