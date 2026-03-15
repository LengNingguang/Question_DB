# CPHOS Question Bank V1

这是一个面向 CPHOS 内部题目整理、统计与检索的轻量题库源码仓。仓库本身只保存 schema、脚本、API、样例数据和文档；生产 SQLite、生产 assets、原始资料和运行日志放在你们自己的服务器上，并通过环境变量注入路径。

## 仓库职责与服务器职责

源码仓包含：
- `question_bank/`: 核心 Python 包，包含 schema、导入、统计、导出、难度评分和查询仓储。
- `scripts/`: 命令行脚本入口。
- `api/`: FastAPI 应用。
- `samples/demo_bundle/`: 样例清洗包，可直接用于本地演示。
- `assets/`: 样例图片或开发环境静态资源。
- `raw/`: 开发环境原始资料示例区。
- `docs/`: 字段字典、录入规范、维护手册、部署说明、FAQ。
- `tests/`: 围绕样例库的测试。

服务器包含：
- 生产 SQLite 数据库文件。
- 生产题图/附图目录。
- 生产原始资料目录。
- FastAPI 运行进程与日志。
- 备份、回滚和服务重启脚本。

## 配置方式

生产路径不写死在仓库中，通过环境变量注入：

- `QUESTION_BANK_DB_PATH`: 服务器上 SQLite 文件路径。
- `QUESTION_BANK_ASSETS_DIR`: 服务器上题图资产根目录。
- `QUESTION_BANK_RAW_DIR`: 服务器上原始资料目录。

本地未设置这些变量时，项目会回退到仓库内的样例路径，便于开发和测试。

## 快速开始

1. 初始化本地样例数据库

```bash
python scripts/init_db.py
```

2. 校验并导入样例清洗包

```bash
python scripts/validate_bundle.py samples/demo_bundle
python scripts/import_bundle.py samples/demo_bundle --commit
```

3. 导入样例成绩统计并计算难度

```bash
python scripts/import_stats.py samples/demo_bundle/stats/raw_scores.csv --stats-source sample_scores --stats-version demo-v1
python scripts/calculate_difficulty.py --method-version demo-baseline
```

4. 导出内部版与半公开版数据

```bash
python scripts/export_data.py --format jsonl
python scripts/export_data.py --format csv --public
```

5. 执行质量检查

```bash
python scripts/check_data_quality.py
```

## 服务器模式示例

在服务器上先注入环境变量，再运行脚本或 API：

```bash
export QUESTION_BANK_DB_PATH=<SERVER_DB_PATH>
export QUESTION_BANK_ASSETS_DIR=<SERVER_ASSETS_DIR>
export QUESTION_BANK_RAW_DIR=<SERVER_RAW_DIR>
uvicorn api.main:app --host 0.0.0.0 --port 8000
```

脚本仍然保留 `--db-path` 参数；如果同时提供了参数和环境变量，命令行参数优先。

## API 启动

当前仓库未附带 FastAPI 依赖。若要启动 API，请先安装依赖：

```bash
pip install -r requirements.txt
uvicorn api.main:app --reload
```

可用接口：`/health`、`/papers`、`/questions`、`/questions/{question_id}`、`/search`。

## 样例内容

项目自带 3 道示例题：

- 理论题：滑块-斜面动力学
- 实验题：单摆测重力加速度
- 实验题：加热电阻的能量评估

这些样例仅用于本地开发验证，不代表生产库位置或生产部署方式。
