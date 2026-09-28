# Freebuff2API

> 中文文档（Rust 版）。English version: [README_en.md](README_en.md)

Freebuff2API 将 [Freebuff](https://freebuff.com) 免费层逆向为 **OpenAI 兼容** 与 **Anthropic 兼容** 的本地 API 网关。**Rust(axum) 实现**，单二进制零依赖，可在任意 OpenAI/Claude 客户端（Claude Code、Codex、Cursor、LobeChat 等）中使用 Freebuff 免费模型。

## 核心特性

- **双协议出口** — `POST /v1/chat/completions`（OpenAI，流式/非流式）+ `POST /v1/messages`（Claude），适配任意 OpenAI SDK。
- **多账号智能轮询** — 多 Bearer token / web Cookie，健康评分 + 冷却熔断 + 最优账号选择。
- **双桶并发信号量** — 逆向自桌面端并已落地（v0.8）：免费 `{槽:1, 并发:3}`、订阅 `{槽:3, 并发:8}`，网关全局级。每个请求同时占用"槽"与"并发"各一，**实际并发上限 = 槽位容量**（免费层 1、订阅层 3），超时 2s 返回 429。
- **会话保活** — 45s 心跳 + 广告刷新延长额度；排队返回 Retry-After；401 自动冷却。
- **思考程度降级** — 逆向自上游 efforts 字段：glm/deepseek 支持 `low/high/max`，solar/minimax/mimo 不支持自动剥离；Codex 选超范围 effort 自动降级。
- **余额/积分查询** — `GET /api/account/balance`：freebucks 积分、每模型每日剩余、套餐、地区限制。
- **token 一键导入** — 粘贴 curl / HAR / Cookie 串自动解析入库；桌面版托盘「一键登录」内置浏览器自动抓 Cookie。
- **web 版协议适配** — `POST /api/chat/stream`（Cookie 鉴权 SSE 11 事件）、多模态上传、工具调用映射。
- **用量统计** — SQLite 记录请求/token/延迟/错误 + 内置控制面板（`/ui`）。
- **桌面安装包** — Electron 壳自动拉起网关 + 托盘 + OAuth 一键登录 + 检查更新。
- **Docker / CI** — 多阶段镜像 + GitHub Actions 自动构建安装包。

## 快速开始

### 桌面版（推荐）
1. 下载最新版 `Freebuff2API Setup x64.exe`（Release 页，当前 v0.8.x）
2. 安装后双击 → 自动拉起网关 + 打开控制台
3. 托盘「一键登录新账号」→ 浏览器登录 freebuff.com → 自动抓 Cookie 入库

### 源码
```bash
build.bat                      # Windows 编译
./target/release/freebuff2api  # Linux/macOS 编译 cargo build --release
start.bat                      # Windows 启动
```

### Docker
```bash
docker build -t freebuff2api -f docker/Dockerfile .
docker run -d --name freebuff2api \
  -p 47821:47821 \
  -e PORT=47821 \
  -e API_KEYS=sk-local \
  -e AUTH_TOKENS=你的freebuff凭证 \
  -v freebuff2api-data:/data \
  freebuff2api
```
容器默认监听 `0.0.0.0`，数据全部落在 `/data`（由 `DATA_DIR` 决定）。不设 `API_KEYS` 时入口脚本会自动生成一个并打印到容器日志。

### Railway / 其他 PaaS

本仓库已经**为 Railway 做过适配**：读平台注入的 `PORT`、数据统一落 `/data` 卷、入口自动补 `api_keys`、
健康检查走 `/healthz`、CSRF 改成同源对账以兼容自定义域名。

**完整步骤与排错见 → [docs/RAILWAY.md](docs/RAILWAY.md)**

最短路径：
1. Railway → New Project → Deploy from GitHub repo（`railway.json` 已指定 Dockerfile 构建 + `/healthz` 健康检查）
2. Settings → **Volumes** → 挂载路径填 **`/data`**（必做，否则每次部署数据清空）
3. Variables 里按需设 `AUTH_TOKENS`；首次部署可以先不设，起来后在 `/ui` 面板导入凭证

## 配置（config.json）

```jsonc
{
  "listen_addr": "127.0.0.1:47821",
  "upstream_base_url": "https://www.codebuff.com",
  "auth_tokens": ["bearer-token-1", "bearer-token-2"],
  "api_keys": ["sk-local"],
  "http_proxy": "http://127.0.0.1:10808",
  "ad_providers": ["gravity"],
  "sqlite_path": "data/freebuff2api.sqlite",
  "token_saver": false,
  "memory_enabled": false
}
```

环境变量优先：`AUTH_TOKENS` / `API_KEYS` / `HTTP_PROXY` / `LISTEN_ADDR` / `PORT` / `DATA_DIR` / `SKIP_UPSTREAM_CHECK` / `AD_PROVIDERS` / `SQLITE_PATH` / `MEMORY_ENABLED`。

> `PORT` 是容器/PaaS 注入端口的通用约定（Railway / Render / Fly / Heroku 都叫这个名字）：
> 它只覆盖端口，且原 host 是回环时会被改成 `0.0.0.0`。
> `DATA_DIR` 把全部相对数据路径（`data/*.sqlite` 等）统一挪到该目录下，容器里指向持久卷。
> 部署相关变量与排错见 [docs/RAILWAY.md](docs/RAILWAY.md)。

### 记忆层（可选，默认关闭）

网关内置一个**零 LLM 规则**的本地记忆库（SQLite，`data/memory.sqlite`）：纯确定性规则自动记录「常用模型 / 推理档位降级 / 你的纠正（"记住…"、"别再…"、"always/never"）」与手动条目，并在相关对话时以低权威注入 system 前缀。

- **默认关闭**：`memory_enabled: false`。记忆不是每个人都需要的，不需要时保持关闭，请求零额外注入。
- **开启方式**：
  1. 面板「记忆」页顶部 switch 一键开启（`POST /api/memory/toggle`，写回 config.json **立即热生效，无需重启**）；
  2. 或 config.json 设 `"memory_enabled": true` 后重启；
  3. 或环境变量 `MEMORY_ENABLED=true`。
- 关闭状态既不自动记录也不注入任何记忆内容；数据保留在 `data/memory.sqlite`，重新开启后继续可用。

## API

| 端点 | 方法 | 说明 |
|------|------|------|
| `/v1/chat/completions` | POST | OpenAI 聊天 |
| `/v1/messages` | POST | Claude 聊天 |
| `/v1/models` | GET | 模型列表 |
| `/api/tokens/import` | POST | 导入 curl/HAR/Cookie |
| `/api/account/balance` | GET | 账号积分/每模型剩余 |
| `/api/account/detail` | POST | 账号详情卡片 |
| `/api/usage/*` | GET | 用量统计 |
| `/ui` | GET | 控制面板 |
| `/healthz` | GET | 健康检查 |

完整教程见 [docs/API_GUIDE.md](docs/API_GUIDE.md)。

## 多账号轮询与并发
- 每请求自动选健康度最高的账号
- 上游双桶并发限制（v0.8 已落地实现）：免费 `{槽:1, 并发:3}`、订阅 `{槽:3, 并发:8}`（实际并发上限=槽位，见上）
- 等待室：429 + retry-after 自动退避

## 思考程度支持矩阵（逆向自上游）

| 模型 | 支持 efforts |
|------|-------------|
| deepseek/*、z-ai/glm、stealth/ox-alpha | `low, high, max` |
| openai/gpt-5.6*、gemini-3.8、claude-fable-5 | `low, medium, high, xhigh, max` |
| meta/muse-spark* | `minimal, low, medium, high, xhigh` |
| solar-pro4、minimax-m3、mimo-v2.5、kimi-k3 | 不支持（自动剥离） |

## 测试与验证

```bash
cargo test        # 253 单测 + 8 集成 + 11 路由级集成全绿（v0.9.0）
cargo clippy --all-targets -- -D warnings  # 零警告
```

真实 E2E 已实测：token 导入（curl/HAR/Cookie）✅、余额查询 ✅、账号详情 ✅、面板 ✅、上游冒烟 ✅。

## 目录结构

```
src/                Rust 网关源码
  api.rs            HTTP 路由
  web_protocol.rs   web 版协议（Cookie 鉴权 chat/stream/余额）
  import.rs         token 导入解析
  usage.rs          SQLite 用量统计
desktop/            Electron 桌面壳
legacy-go/          旧 Go 版实现（归档）
docker/             容器构建（Dockerfile + entrypoint.sh）
railway.json        Railway 部署配置（Dockerfile 构建 + /healthz 健康检查）
docs/               API 教程 / Railway 部署指南
```

> 上游的 `reference/`（桌面端反编译产物归档）在本仓库已移除：对构建与部署零作用，
> 且公开仓库再分发反编译产物有版权风险。需要时从上游仓库取回。

## 免责声明

本项目与 OpenAI、Codebuff、Freebuff 无官方关联。仅供交流、实验与学习使用，按"原样"提供，使用者自行承担风险。

## 开源协议

MIT
