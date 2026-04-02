# ============================================================
# 文件：scripts/tests/config.py
# 说明：E2E 测试配置文件
# ============================================================

"""
E2E 测试常量与路径配置

定义测试脚本中使用的目录、端口、数据库连接等常量
"""

from pathlib import Path
import os


# ============================================================
# 目录配置
# ============================================================
# 项目根目录：当前文件的上上级目录
ROOT_DIR = Path(__file__).resolve().parents[2]

# 临时目录：存放测试过程中生成的所有文件
TMP_DIR = ROOT_DIR / "tmp"

# 样本目录：存放测试用的 ZIP 文件样本
SAMPLES_DIR = TMP_DIR / "samples"

# 下载目录：存放 API 导出的 ZIP 文件
DOWNLOADS_DIR = TMP_DIR / "downloads"

# API 日志路径：记录 E2E 测试期间 API 的运行日志
API_LOG_PATH = TMP_DIR / "qb_api_e2e.log"

# 导出文件路径：JSONL 格式的题目导出
EXPORT_PATH = TMP_DIR / "qb_e2e_internal.jsonl"

# 质量检查报告路径：JSON 格式
QUALITY_PATH = TMP_DIR / "qb_e2e_quality.json"

# 测试报告路径：Markdown 格式
REPORT_PATH = TMP_DIR / "qb_e2e_report.md"

# 无效试卷上传路径：用于测试错误处理（非 ZIP 文件）
INVALID_PAPER_UPLOAD_PATH = SAMPLES_DIR / "paper_invalid_upload.bin"

# 真实测试 ZIP 路径：理论题样题 (test.zip)
REAL_TEST_ZIP_PATH = ROOT_DIR / "scripts" / "test.zip"

# 真实测试 2 路径：实验题样题 (test2.zip)
REAL_TEST2_ZIP_PATH = ROOT_DIR / "scripts" / "test2.zip"


# ============================================================
# Docker 容器配置
# ============================================================
# 容器名称：可通过环境变量覆盖
CONTAINER_NAME = os.environ.get("CONTAINER_NAME", "qb-postgres-e2e")

# PostgreSQL 镜像：可通过环境变量覆盖
POSTGRES_IMAGE = os.environ.get("POSTGRES_IMAGE", "postgres:14.1")

# 数据库端口：映射到宿主机的端口
POSTGRES_PORT = os.environ.get("POSTGRES_PORT", "55433")

# API 端口：QB 服务绑定的端口
API_PORT = os.environ.get("API_PORT", "18080")

# 数据库连接 URL
DB_URL = f"postgres://postgres:postgres@127.0.0.1:{POSTGRES_PORT}/qb"


"""
============================================================
知识点讲解 (Python 测试配置)
============================================================

1. Path 对象操作
   Path(__file__).resolve().parents[2]
   - __file__: 当前文件路径
   - resolve(): 转换为绝对路径
   - parents[2]: 上上级目录（项目根目录）

2. 环境变量覆盖
   os.environ.get("VAR_NAME", "default_value")
   - 允许通过环境变量自定义配置
   - 适合 CI/CD 或不同测试环境

3. 路径拼接
   ROOT_DIR / "subdir" / "file.txt"
   - 使用 / 运算符拼接路径
   - 自动处理不同操作系统的路径分隔符

============================================================
测试目录结构
============================================================

project_root/
├── scripts/
│   ├── test_full_flow.py      # E2E 入口
│   └── tests/
│       ├── config.py          # 配置（本文件）
│       ├── specs.py           # 测试数据规格
│       ├── fixtures.py        # 测试夹具构建
│       ├── session.py         # 测试会话管理
│       ├── validators.py      # 响应验证函数
│       └── full_flow.py       # 完整测试流程
├── tmp/                       # 测试临时目录
│   ├── samples/              # 生成的测试 ZIP
│   ├── downloads/            # API 导出的 ZIP
│   ├── qb_api_e2e.log        # API 日志
│   ├── qb_e2e_internal.jsonl # 导出题目
│   ├── qb_e2e_quality.json   # 质量报告
│   └── qb_e2e_report.md      # 测试报告
├── test.zip                   # 理论题样题
└── test2.zip                  # 实验题样题

============================================================
端口设计说明
============================================================

POSTGRES_PORT = 55433
  - 使用非常见端口避免冲突
  - Docker 容器暴露 5432 → 宿主机 55433

API_PORT = 18080
  - 非常见 HTTP 端口
  - 避免与开发服务器冲突

DB_URL 格式:
  postgres://用户：密码@主机：端口/数据库名
  postgres://postgres:postgres@127.0.0.1:55433/qb

============================================================
测试文件用途
============================================================

| 文件                      | 用途                           |
|---------------------------|--------------------------------|
| test.zip                  | 6 道真实理论题（文件夹格式）    |
| test2.zip                 | 4 道真实实验题（文件夹格式）    |
| question_*.zip            | 3 道合成题目（测试生成）        |
| paper_appendix_*.zip      | 2 个试卷附录（测试生成）        |
| paper_invalid_upload.bin  | 无效文件（测试错误处理）        |
"""
