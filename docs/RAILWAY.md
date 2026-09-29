# 部署到 Railway

本文档是**为 PaaS 做过适配的部署路径**。本仓库相对上游 `lza6/Freebuff-2API` 的改动全部围绕"容器/PaaS 上能不能一把起来"，逐条见文末《与上游的差异》。

---

## 一、为什么上游版本不能直接上 Railway

四个硬阻塞，缺一个就是部署失败或数据静默丢失：

| # | 阻塞点 | 上游行为 | 在 Railway 上的后果 |
|---|--------|----------|---------------------|
| 1 | 端口 | `listen_addr` 默认 `127.0.0.1:47821`，且**不读 `PORT`** | 平台反向代理从容器外进来，绑回环 = 健康检查永远失败，部署被判失败 |
| 2 | 入口 | `ENTRYPOINT freebuff2api --config /data/config.json` | 卷里没有 `config.json` 时进程**立刻退出** → crash-loop |
| 3 | 数据路径 | 全部是相对路径 `data/*.sqlite` | 落在容器可写层，**每次重新部署数据全丢**（凭证、用量、记忆） |
| 4 | 启动门禁 | 无 `AUTH_TOKENS` 直接 `anyhow!` 退出 | 首次部署还没导凭证 → 起不来 → 连导入入口都进不去 |

另外还有一个只在域名下才暴露的坑：

| # | 问题 | 现象 |
|---|------|------|
| 5 | `origin_allowed` 写死回环 Origin | `/ui` 页面**能打开**，但改配置、导入凭证、生成 Key、切记忆开关**全部 403**。只读页面正常、写操作全挂，极难定位 |

---

## 二、部署步骤

### 1. 建服务

- Railway → **New Project** → **Deploy from GitHub repo** → 选本仓库
- 服务会自动读取根目录的 `railway.json`：
  - builder = `DOCKERFILE`
  - dockerfilePath = `docker/Dockerfile`
  - healthcheck = `/healthz`

> 不用手动改 Build/Start Command，`railway.json` 已经写死。Start Command 保持默认（走镜像 `ENTRYPOINT`）。

### 2. 挂持久卷（**必做**）

服务 → **Settings → Volumes** → 新建卷，**Mount path 填 `/data`**。

不做这步，凭证、用量统计、记忆库会在每次重新部署后清空。

### 3. 设置变量

最小可用集合（其余全有默认值）：

| 变量 | 必填 | 说明 |
|------|------|------|
| `AUTH_TOKENS` | 建议 | Freebuff 凭证，多个用**逗号或换行**分隔。不填也能起来（见下），但要真跑模型必须有 |
| `API_KEYS` | 可选 | 网关对外鉴权 key。**不填会自动生成一个并打在部署日志里** |
| `DATA_DIR` | 否 | 默认 `/data`，与卷挂载点保持一致即可，一般不用改 |
| `LISTEN_ADDR` | 否 | 默认 `0.0.0.0:$PORT`。一般不用改 |
| `HTTP_PROXY` | 否 | 出站代理，支持 `http://` / `socks5://` |
| `MEMORY_ENABLED` | 否 | 记忆层开关，默认 `false` |
| `RUST_LOG` | 否 | 默认 `info`，排查时设 `debug,freebuff2api=trace` |

**首次部署可以先不填任何变量**：入口脚本会自动生成 `API_KEYS` 并把明文打进 Deploy Logs，同时以空账号池启动（`SKIP_UPSTREAM_CHECK=true`）。起来后在面板里导入凭证即可，不用重启。

### 4. 冒烟

```bash
curl -s https://<你的域名>/healthz            # {"ok":true,...}
curl -s -H "Authorization: Bearer <API_KEYS>" https://<你的域名>/v1/models | head -c 200
```

浏览器打开 `https://<你的域名>/ui`，在面板顶部的 Key 输入框填上 `API_KEYS`。

---

## 三、端口与监听的行为约定

`src/config.rs` 的解析顺序（环境变量优先于 `config.json`）：

1. 先按 `LISTEN_ADDR`（或配置文件里的 `listen_addr`）得到初始值
2. 若存在 `PORT` 环境变量 → **保留 host，只换端口**；且 **host 是回环时强制改成 `0.0.0.0`**

第 2 条是刻意的：容器里绑 `127.0.0.1` 一定不可达，平台健康检查必然失败。保留显式非回环 host（如 `192.168.1.10`、`[2001:db8::1]`）不变。

> 想在容器里真的只监听回环（比如同 Pod 内 sidecar 访问），请显式设 `PORT=` 为空 —— 不要靠改 `LISTEN_ADDR`，它会被 `PORT` 覆盖。

---

## 四、安全模型（部署后请过一眼）

- **非回环监听 + 空 `api_keys` 会被程序硬拒绝**（`config.rs::validate`）。入口脚本自动生成的 key 就是为了绕开这个门禁**同时不裸奔**。
- 数据面（`/v1/*`）与管理面（`/api/*`、`/ui`）共用 `API_KEYS`：`Authorization: Bearer <key>` 或 `x-api-key: <key>`。
- CSRF 防护走**同源对账**（Origin 的 host 与请求 Host 一致才放行）。所以换成自定义域名后不用再改代码；但如果你在前面又套了一层 CDN 并改写了 `Host`，要保证 `Origin` 和 `Host` 仍然一致。
- `redact_logs` 默认 `true`：写日志/遥测前把 Cookie、Bearer、authorization 的值替换掉。
- SQLite 使用的不是本地文件锁的安全场景：**多副本会同时写同一个卷上的库文件**。`railway.json` 没开多副本，也请不要手动加；要横向扩就得先把存储换成外部数据库。

---

## 五、构建耗时

首次构建要从零编译 Rust 依赖（axum / tokio / reqwest / rusqlite bundled），**大约 8–15 分钟**，这是正常的。

`docker/Dockerfile` 里已经用"空壳 main/lib 预编译依赖"做了分层缓存：只要 `Cargo.toml` / `Cargo.lock` 没变，后续部署这层直接命中，**大约 1–3 分钟**。

> 改了 `Cargo.toml` 的依赖（含版本号）会让缓存层失效，回到满速构建。

---

## 六、常见问题

**健康检查一直失败 / 部署被判定失败**
先看 Deploy Logs 有没有 `[entrypoint]` 那几行。
- 日志停在 `未设置 API_KEYS` 之后 → 大概率是 `/data` 目录不可写，检查卷挂载路径是不是 `/data`
- 完全没有日志 → 镜像没构建成功，翻 Build Logs

**面板能打开，但点任何按钮都报 403**
`API_KEYS` 没填对。面板顶部的 Key 输入框里要填**部署时生成/设置的那个 key**（存在浏览器 localStorage 的 `freebuff_api_key` 里）。也可以在 DevTools 里 `localStorage.removeItem('freebuff_api_key')` 清掉重填。

**重新部署后凭证没了**
没挂卷，或卷的 Mount path 不是 `/data`。确认变量 `DATA_DIR` 与卷挂载点一致。

**`API_KEYS` 每次重新部署都变**
自动生成的 Key 会写入 `/data/.auto_api_key` 持久化 —— 挂了卷就跨部署不变（没挂卷时内存级随机，重启即换）。要彻底固定就在 Railway 变量里显式设 `API_KEYS`（优先级高于自动生成）。

**「生成并启用 Key」按钮报 unauthorized**
这是管理端点，需要先用**当前生效的 Key**（Deploy Logs 里 `sk-fb-` 开头那把）粘到面板右上角登录，才能生成新的。不是 bug：公网下若允许匿名重置 Key，任何人都能劫持你的网关。

**想用自己的 config.json**
把它放到卷里（`/data/config.json`）。存在时入口脚本改用 `--config /data/config.json` 启动，环境变量仍然优先覆盖。注意容器里 `listen_addr` 要写 `0.0.0.0`，路径建议用 `/data/...` 绝对路径。

---

## 七、与上游的差异清单

| 文件 | 改动 |
|------|------|
| `src/config.rs` | 新增 `PORT` 解析（回环 host 自动改 `0.0.0.0`）；新增 `DATA_DIR`（相对路径统一并入卷目录）；新增 `SKIP_UPSTREAM_CHECK` 环境变量；抽出 `host_of` / `is_loopback_host` / `rewrite_port` / `under_data_dir` / `is_absolute_path` 并补 6 组单测 |
| `src/api.rs` | `origin_allowed` 从"写死 127.0.0.1/localhost"改为**与请求 Host 做同源对账**，保留 `file://` 与浏览器扩展例外；补 1 组单测 |
| `docker/Dockerfile` | 入口换成 `entrypoint.sh`；`HEALTHCHECK` 用 `$PORT`；默认 `0.0.0.0`；移除 `VOLUME`；加 `--locked`；运行阶段补 `openssl` |
| `docker/entrypoint.sh` | 新增：建数据目录 → 补 `API_KEYS`（自动生成 + 打印）→ 无凭证时 `SKIP_UPSTREAM_CHECK=true` → exec 网关 |
| `railway.json` | 新增：Dockerfile builder + `/healthz` 健康检查 + 失败重启策略 |
| `.dockerignore` | 收窄构建上下文（排除测试脚本、Electron 壳、CF Worker、Go 归档等） |
| `.gitattributes` | 新增：统一 LF，防 shell 脚本被 checkout 成 CRLF 后在容器里报 "not found" |
| `.github/workflows/docker.yml` | GHCR 镜像名改用 `${{ github.repository_owner }}`，不再是上游所有者 |
| `README.md` / `README_RUST.md` | 补 Railway 入口指向本文档；目录结构同步 |
| `reference/`、`计划书/`、各 AI Agent 配置目录 | **已移除**（详见下） |

### 被移除的内容

| 路径 | 原因 |
|------|------|
| `reference/`（40 MB） | 上游桌面端**反编译产物**归档。公开仓库再分发有版权风险，且对构建/部署零作用 |
| `计划书/` | 项目内部规划文档，与部署无关 |
| `.agents/` `.claude/` `.cline/` `.codebuddy/` `.codegraph/` `.continue/` `.junie/` `.kiro/` `.mcp.json` `opencode.json` `AGENTS.md` `skills-lock.json` | 各类 AI 编码工具的本地配置，与运行无关 |

需要恢复这些内容，从上游 `lza6/Freebuff-2API` 对应提交里取回即可。
