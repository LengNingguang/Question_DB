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
- `question_difficulties`
  保存每个 difficulty tag 的 `score` / `notes`。
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
- 上传题目时必须额外提供一个非空的 `description`
- 上传题目时必须额外提供一个 `difficulty` JSON 字符串，且至少包含 `human`
- `description` 支持中文，并可直接参与 `GET /questions?q=...` 的模糊匹配
- `description` 会参与 bundle 目录命名，因此不能包含 `/ \\ : * ? " < > |`，不能是 `.`、`..`，也不能以 `.` 结尾
- 题目 metadata 在上传时其余字段使用默认值：
  - `category = "none"`
  - `tags = []`
  - `status = "none"`
  - `created_at = NOW()`

## 核心 API

### 上传题目

`POST /questions`

请求示例：

```bash
curl -X POST http://127.0.0.1:8080/questions \
  -F "description=热学标定题" \
  -F 'difficulty={"human":{"score":5,"notes":"baseline"}}' \
  -F "file=@question.zip"
```

### 更新题目 metadata

`PATCH /questions/{question_id}`

请求体示例：

```json
{
  "category": "T",
  "description": "demo question",
  "tags": ["optics", "mechanics"],
  "status": "reviewed",
  "difficulty": {
    "human": {
      "score": 7,
      "notes": "sample"
    },
    "algo1": {
      "score": 6
    }
  }
}
```

说明：

- 请求体支持部分更新
- `description` 如果出现在更新请求里，必须是非空字符串，并满足上面的文件名安全限制
- `tags` 传空数组会清空
- `difficulty` 如果出现在更新请求里，会整体替换整组 difficulty tag
- `difficulty` 必须至少包含 `human`
- `difficulty.<tag>.score` 要求在 `1..=10`
- `difficulty.<tag>.notes` 是可选字符串，空串会被规范化为 `null`
- `category` 只能是 `none`、`T`、`E`
- `status` 只能是 `none`、`reviewed`、`used`

### 删除题目

`DELETE /questions/{question_id}`

### 批量下载题目包

`POST /questions/bundles`

请求体示例：

```json
{
  "question_ids": [
    "8db0d12e-2968-4ede-86d5-1dc5ff0a5d10",
    "e21ed70d-cd18-45cc-89ab-2785d07f4de7"
  ]
}
```

返回一个 zip，根目录附带 `manifest.json`，并按 `description_uuid前缀/` 目录分组题目文件。

### 创建试卷

`POST /papers`

使用 `multipart/form-data` 上传初始化 metadata 和附加 zip。

示例：

```bash
curl -X POST http://127.0.0.1:8080/papers \
  -F 'description=demo paper' \
  -F 'title=Demo Paper' \
  -F 'subtitle=Initial Draft' \
  -F 'authors=["Alice","Bob"]' \
  -F 'reviewers=["Carol"]' \
  -F 'question_ids=["8db0d12e-2968-4ede-86d5-1dc5ff0a5d10","e21ed70d-cd18-45cc-89ab-2785d07f4de7"]' \
  -F 'file=@paper_appendix.zip;type=application/zip'
```

题目顺序由 `question_ids` 数组顺序决定。

说明：

- `description` 为必填，必须是非空字符串
- `description` 同样会参与 bundle 目录命名，因此也要满足上面的文件名安全限制
- `title`、`subtitle` 为必填非空字符串
- `authors`、`reviewers` 为 JSON 字符串数组
- `POST /papers` 必须带一个合法 zip，服务端会原样存成附加 binary
- `GET /papers` 支持通过 `question_id`、`category`、`tag`、`q`、`limit`、`offset` 做组合查询
- `q` 会模糊匹配试卷的 `description`、`title`、`subtitle`、`authors`、`reviewers`

### 更新试卷

`PATCH /papers/{paper_id}`

更新 metadata，也支持通过 `question_ids` 重排题目列表。支持字段：`description`、`title`、`subtitle`、`authors`、`reviewers`、`question_ids`。

### 删除试卷

`DELETE /papers/{paper_id}`

### 批量下载试卷包

`POST /papers/bundles`

请求体示例：

```json
{
  "paper_ids": [
    "79bf1ce3-6b61-4e5f-82ab-29e5c7cb2942",
    "8ff430a0-92aa-463b-bf0f-b244a6697bf4"
  ]
}
```

返回一个 zip，根目录附带 `manifest.json`，并按 `paperDescription_uuid前缀/questionDescription_uuid前缀/` 目录展开题目文件。每个试卷目录还会附带一个重命名后的 `append.zip`，内容就是创建试卷时上传的那个 zip。

### 查询与运维

- `GET /papers`
- `GET /papers/{paper_id}`
- `POST /papers/bundles`
- `GET /questions`
- `GET /questions/{question_id}`
- `POST /questions/bundles`
- `POST /exports/run`
- `POST /quality-checks/run`

`GET /questions` 额外支持：

- `difficulty_tag`
- `difficulty_min`
- `difficulty_max`

其中 `difficulty_min` / `difficulty_max` 需要和 `difficulty_tag` 一起使用。

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
