# CPHOS Question Bank

Rust + Axum + PostgreSQL 题库服务。当前版本的核心流程是：

1. 上传单题 zip 压缩包导入题目
2. 用一组有序题目引用创建试卷

## 项目架构

```text
src/
├── api/
│   ├── mod.rs
│   ├── questions/
│   │   ├── API.md
│   │   ├── handlers.rs
│   │   ├── imports.rs
│   │   ├── models.rs
│   │   └── queries.rs
│   ├── papers/
│   │   ├── API.md
│   │   ├── handlers.rs
│   │   └── models.rs
│   ├── ops/
│   │   ├── API.md
│   │   ├── exports.rs
│   │   ├── handlers.rs
│   │   ├── models.rs
│   │   └── quality.rs
│   ├── system/
│   │   ├── API.md
│   │   └── handlers.rs
│   └── shared/
│       ├── error.rs
│       └── utils.rs
├── config.rs
├── db.rs
├── lib.rs
└── main.rs
```

## 数据模型

- `objects`
  单表保存任意上传文件的元数据与二进制内容。
- `questions`
  保存题目固定 metadata。
- `question_files`
  保存题目的 TeX 文件和资源文件引用。
- `question_tags`
  保存题目标签列表。
- `question_difficulty_algorithms`
  保存算法打分列表。
- `papers`
  保存试卷固定元数据。
- `paper_questions`
  保存试卷和题目的有序关联。

## 题目 zip 格式

上传文件大小限制 20 MiB，使用 `multipart/form-data`，字段名是 `file`。

zip 根目录下必须是标准题目录入包格式：

```text
question.zip
├── problem.tex
└── assets/
    ├── figure1.png
    └── ...
```

其中：

- zip 根目录必须恰好有一个 `.tex` 文件
- zip 根目录必须恰好有一个 `assets/` 目录
- 除根目录 tex 和 `assets/` 下资源外，不允许额外文件或目录
- tex 和 `assets/` 下的资源文件都会写入 `objects` 表
- 题目 metadata 在上传时使用默认值：
  - `category = "none"`
  - `notes = ""`
  - `tags = []`
  - `status = "none"`
  - `difficulty = {}`
  - `created_at = NOW()`

## 核心 API

### 上传题目

`POST /questions`

请求示例：

```bash
curl -X POST http://127.0.0.1:8080/questions \
  -F "file=@question.zip"
```

### 更新题目 metadata

`PATCH /questions/{question_id}`

请求体示例：

```json
{
  "category": "T",
  "notes": "demo question",
  "tags": ["optics", "mechanics"],
  "status": "reviewed",
  "difficulty": {
    "human": 7,
    "algorithm": {
      "algo1": 6
    },
    "notes": "sample"
  }
}
```

说明：

- 请求体支持部分更新
- `notes` 传 `null` 或空串会被清空为 `""`
- `tags` 传空数组会清空
- `difficulty` 传 `{}` 会清空整个难度信息
- `difficulty.human` 和 `difficulty.algorithm.*` 都要求在 `1..=10`
- `category` 只能是 `none`、`T`、`E`
- `status` 只能是 `none`、`reviewed`、`used`

### 删除题目

`DELETE /questions/{question_id}`

### 创建试卷

`POST /papers`

请求体示例：

```json
{
  "edition": "2026",
  "paper_type": "regular",
  "title": "CPHOS Mock Paper",
  "notes": "demo",
  "question_ids": [
    "8db0d12e-2968-4ede-86d5-1dc5ff0a5d10",
    "e21ed70d-cd18-45cc-89ab-2785d07f4de7"
  ]
}
```

题目顺序由 `question_ids` 数组顺序决定。

### 更新试卷

`PATCH /papers/{paper_id}`

### 删除试卷

`DELETE /papers/{paper_id}`

### 查询与运维

- `GET /papers`
- `GET /papers/{paper_id}`
- `GET /questions`
- `GET /questions/{question_id}`
- `POST /exports/run`
- `POST /quality-checks/run`

## 启动

```bash
export QB_DATABASE_URL='postgres://postgres:postgres@127.0.0.1:5432/qb'
export QB_BIND_ADDR='127.0.0.1:8080'
psql "$QB_DATABASE_URL" -f migrations/0001_init_pg.sql
cargo run
```

## 测试

单元与集成测试：

```bash
cargo test
```

端到端脚本：

```bash
python3 scripts/test_full_flow.py
```

## 数据库格式

表结构定义在 [0001_init_pg.sql](/home/be/Question_DB/migrations/0001_init_pg.sql)。
