# 部署说明

本项目采用“源码仓 + 服务器 SQLite”模式。

## 1. 服务器准备
- 代码目录：`<APP_ROOT>`
- 数据库路径：`<SERVER_DB_PATH>`
- 题图目录：`<SERVER_ASSETS_DIR>`
- 原始资料目录：`<SERVER_RAW_DIR>`
- 服务名：`<SERVICE_NAME>`
- 监听端口：`<SERVICE_PORT>`

## 2. 拉取源码并安装依赖
```bash
cd <APP_ROOT>
git pull
python -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
```

## 3. 注入环境变量
可以通过 systemd、supervisor 或私有 `.env` 文件注入：

```bash
export QUESTION_BANK_DB_PATH=<SERVER_DB_PATH>
export QUESTION_BANK_ASSETS_DIR=<SERVER_ASSETS_DIR>
export QUESTION_BANK_RAW_DIR=<SERVER_RAW_DIR>
```

## 4. 初始化或更新数据库
- 首次部署时初始化数据库 schema。
- 后续通过导入脚本把清洗包写入服务器上的生产库。

```bash
python scripts/init_db.py --db-path <SERVER_DB_PATH>
python scripts/import_bundle.py <BUNDLE_PATH> --db-path <SERVER_DB_PATH> --commit
```

## 5. 启动 API
```bash
uvicorn api.main:app --host 0.0.0.0 --port <SERVICE_PORT>
```

## 6. 服务守护
推荐使用 `systemd` 或等价服务管理工具，并将日志写入服务器本地日志目录。不要把真实日志路径或服务账户写进 Git。

## 7. 备份建议
- 定期备份 `<SERVER_DB_PATH>`。
- 对 `<SERVER_ASSETS_DIR>` 做目录级备份。
- 每次大规模导入后额外导出内部 JSONL 快照。

## 8. 回滚建议
- 代码问题：回滚源码仓版本并重启服务。
- 数据问题：回滚服务器数据库备份，再重新导入缺失内容。
