# CPHOS Question_DB 项目架构详解

## 一、项目概述

这是一个用 **Rust + Axum + PostgreSQL** 开发的**题库管理系统**，主要用于管理物理竞赛题目和试卷。

### 核心技术栈
- **Web 框架**: Axum 0.7 (基于 Tokio 的异步 Web 框架)
- **数据库**: PostgreSQL + SQLx (异步 SQL 库)
- **序列化**: Serde + Serde JSON
- **并发**: Tokio 异步运行时
- **日志**: Tracing + tracing-subscriber

---

## 二、项目目录结构

```
cphos/
├── Cargo.toml              # Rust 项目配置（依赖管理）
├── Cargo.lock              # 依赖版本锁定文件
├── README.md               # 项目说明文档
├── migrations/             # 数据库迁移脚本
│   └── 0001_init_pg.sql   # 数据库表结构定义
├── scripts/                # Python 测试脚本
│   ├── test_full_flow.py   # 端到端测试
│   └── tests/              # 测试模块
│       ├── config.py       # 测试配置
│       ├── fixtures.py     # 测试夹具
│       ├── full_flow.py    # 完整流程测试
│       ├── session.py      # 会话管理
│       ├── specs.py        # 测试规格
│       └── validators.py   # 验证器
├── CPHOS-Latex/            # LaTeX 模板目录（用于试卷导出）
├── src/                    # Rust 源代码
│   ├── main.rs             # 程序入口点
│   ├── lib.rs              # 库入口（模块导出）
│   ├── config.rs           # 配置加载（环境变量）
│   ├── db.rs               # 数据库连接池创建
│   └── api/                # API 层
│       ├── mod.rs          # API 模块总入口
│       ├── shared/         # 共享工具
│       │   ├── error.rs    # 统一错误处理
│       │   └── utils.rs    # 工具函数
│       ├── system/         # 系统接口
│       │   ├── API.md      # 接口文档
│       │   └── handlers.rs # 健康检查等
│       ├── questions/      # 题目管理
│       │   ├── API.md      # 接口文档
│       │   ├── handlers.rs # HTTP 请求处理
│       │   ├── models.rs   # 数据模型定义
│       │   ├── imports.rs  # ZIP 导入逻辑
│       │   └── queries.rs  # 数据库查询
│       ├── papers/         # 试卷管理
│       │   ├── API.md      # 接口文档
│       │   ├── handlers.rs # HTTP 请求处理
│       │   ├── models.rs   # 数据模型定义
│       │   └── imports.rs  # ZIP 导入逻辑
│       └── ops/            # 运维接口
│           ├── API.md      # 接口文档
│           └── handlers.rs # 导出/质量检查
└── tests/                  # Rust 集成测试
    └── health_route.rs     # 健康检查测试
```

---

## 三、数据库设计

数据库共有 **7 张表**，定义在 `migrations/0001_init_pg.sql`：

### 1. objects（对象存储表）
存储所有上传文件的二进制内容。
| 字段 | 类型 | 说明 |
|------|------|------|
| object_id | UUID | 主键 |
| file_name | TEXT | 文件名 |
| mime_type | TEXT | MIME 类型 |
| size_bytes | BIGINT | 文件大小 |
| content | BYTEA | 二进制内容 |
| created_at | TIMESTAMPTZ | 创建时间 |

### 2. questions（题目表）
存储题目元数据。
| 字段 | 类型 | 说明 |
|------|------|------|
| question_id | UUID | 主键 |
| source_tex_path | TEXT | TeX 文件路径 |
| category | TEXT | 类别：none/T(理论)/E(实验) |
| status | TEXT | 状态：none/reviewed(已审核)/used(已使用) |
| description | TEXT | 题目描述 |
| created_at | TIMESTAMPTZ | 创建时间 |
| updated_at | TIMESTAMPTZ | 更新时间 |

### 3. question_files（题目文件表）
关联题目与 objects 表，记录 TeX 和资源文件。
| 字段 | 类型 | 说明 |
|------|------|------|
| question_file_id | UUID | 主键 |
| question_id | UUID | 外键→questions |
| object_id | UUID | 外键→objects |
| file_kind | TEXT | 类型：tex(题目)/asset(资源) |
| file_path | TEXT | 文件路径 |
| mime_type | TEXT | MIME 类型 |

### 4. question_tags（题目标签表）
| 字段 | 类型 | 说明 |
|------|------|------|
| question_id | UUID | 外键→questions |
| tag | TEXT | 标签名 |
| sort_order | INT | 排序顺序 |

### 5. question_difficulties（题目难度表）
| 字段 | 类型 | 说明 |
|------|------|------|
| question_id | UUID | 外键→questions |
| algorithm_tag | TEXT | 算法标签（如 human） |
| score | INT | 难度分数 1-10 |
| notes | TEXT | 备注 |

### 6. papers（试卷表）
| 字段 | 类型 | 说明 |
|------|------|------|
| paper_id | UUID | 主键 |
| description | TEXT | 试卷描述 |
| title | TEXT | 标题 |
| subtitle | TEXT | 副标题 |
| authors | TEXT[] | 作者数组 |
| reviewers | TEXT[] | 审核人数组 |
| append_object_id | UUID | 附加 zip 文件 ID |

### 7. paper_questions（试卷题目关联表）
| 字段 | 类型 | 说明 |
|------|------|------|
| paper_id | UUID | 外键→papers |
| question_id | UUID | 外键→questions |
| sort_order | INT | 题目顺序 |

---

## 四、核心功能流程

### 1. 上传题目（POST /questions）

```
用户上传 question.zip
       ↓
解析 multipart 表单
       ↓
校验 ZIP 格式：
  - 根目录恰好 1 个.tex 文件
  - 根目录恰好 1 个 assets/目录
  - 大小≤20MB
       ↓
解压并验证文件结构
       ↓
写入数据库：
  1. 插入 objects 表（tex+assets）
  2. 插入 questions 表
  3. 插入 question_files 表
  4. 插入 question_difficulties 表
       ↓
返回 question_id
```

### 2. 创建试卷（POST /papers）

```
用户上传试卷信息 + appendix.zip
       ↓
解析 multipart 表单字段：
  - description, title, subtitle
  - authors[], reviewers[]
  - question_ids[]
       ↓
验证题目：
  - 所有题目 category 必须同为 T 或 E
  - 所有题目 status 必须是 reviewed 或 used
       ↓
写入数据库：
  1. 插入 appendix 到 objects 表
  2. 插入 papers 表
  3. 插入 paper_questions 表（按顺序）
       ↓
返回 paper_id
```

### 3. 批量打包下载（POST /questions/bundles）

```
接收 question_ids[]
     ↓
查询题目信息
     ↓
生成 ZIP：
  - manifest.json（清单）
  - 每个题目一个目录：description_uuid/
    - 原始.tex 文件
    - assets/资源目录
     ↓
返回 application/zip
```

---

## 五、API 接口一览

### 题目管理 (Questions)
| 方法 | 路径 | 功能 |
|------|------|------|
| POST | /questions | 上传单题 ZIP |
| GET | /questions | 查询题目列表 |
| GET | /questions/{id} | 获取题目详情 |
| PATCH | /questions/{id} | 更新题目元数据 |
| PUT | /questions/{id}/file | 替换题目文件 |
| DELETE | /questions/{id} | 删除题目 |
| POST | /questions/bundles | 批量打包下载 |

### 试卷管理 (Papers)
| 方法 | 路径 | 功能 |
|------|------|------|
| POST | /papers | 创建试卷 |
| GET | /papers | 查询试卷列表 |
| GET | /papers/{id} | 获取试卷详情 |
| PATCH | /papers/{id} | 更新试卷信息 |
| PUT | /papers/{id}/file | 替换附加 ZIP |
| DELETE | /papers/{id} | 删除试卷 |
| POST | /papers/bundles | 批量打包下载 |

### 运维接口 (Ops)
| 方法 | 路径 | 功能 |
|------|------|------|
| POST | /exports/run | 导出题目数据 (JSONL) |
| POST | /quality-checks/run | 运行数据质量检查 |

### 系统接口 (System)
| 方法 | 路径 | 功能 |
|------|------|------|
| GET | /health | 健康检查 |

---

## 六、启动方式

```bash
# 1. 设置环境变量
export QB_DATABASE_URL='postgres://postgres:postgres@127.0.0.1:5432/qb'
export QB_BIND_ADDR='127.0.0.1:8080'

# 2. 初始化数据库
psql "$QB_DATABASE_URL" -f migrations/0001_init_pg.sql

# 3. 启动服务
cargo run
```

---

## 七、关键设计特点

### 1. ZIP 格式校验
- 根目录只能有 1 个.tex 文件和 1 个 assets/目录
- 禁止路径遍历攻击（..、绝对路径）
- 限制 20MB 上传大小
- 限制解压后 64MB

### 2. 事务安全
所有数据库操作使用事务，确保：
- 题目导入时，文件+ 元数据+ 难度同时写入
- 更新操作要么全成功，要么全回滚

### 3. 软删除支持
通过`ON DELETE CASCADE` 外键约束，删除题目时自动清理关联文件。

### 4. 难度评价体系
支持多维度难度评分：
- `human`: 人工评分（必填）
- `heuristic`: 启发式算法评分
- `ml`: 机器学习模型评分
每个维度独立记录分数 (1-10) 和备注。

---

## 八、测试方法

### 单元测试
```bash
cargo test
```

### 端到端测试
```bash
python3 scripts/test_full_flow.py
```

---

## 九、文件用途速查

| 文件 | 用途 |
|------|------|
| `main.rs` | 程序入口，启动 HTTP 服务器 |
| `lib.rs` | 模块导出 |
| `config.rs` | 从环境变量读取配置 |
| `db.rs` | 创建数据库连接池 |
| `api/mod.rs` | 路由组合 |
| `api/shared/error.rs` | 统一错误类型 |
| `api/questions/handlers.rs` | 题目 HTTP 处理逻辑 |
| `api/questions/models.rs` | 题目数据结构 |
| `api/questions/imports.rs` | ZIP 解析与导入 |
| `api/papers/handlers.rs` | 试卷 HTTP 处理逻辑 |

---

*文档生成时间：2026-04-02*
