# CPHOS Question Bank

Rust + Axum + PostgreSQL 题库服务，当前版本采用“独立题目上传 + 试卷显式组装”的核心模型。

## 项目架构

```text
src/
├── api/
│   ├── mod.rs        # 路由入口
│   ├── handlers.rs   # HTTP 编排层
│   ├── models.rs     # 请求/响应模型
│   ├── queries.rs    # 查询规划与结果映射
│   ├── imports.rs    # 题目源校验与导入
│   ├── exports.rs    # 导出
│   ├── quality.rs    # 质量检查
│   ├── utils.rs      # 文件与文本工具
│   └── error.rs      # API 错误响应
├── config.rs
├── db.rs
├── lib.rs
└── main.rs
```

## 核心数据模型

- `questions`：独立题目主数据，只保存题目自己的 metadata。
- `question_assets`：题目资源引用。
- `papers`：试卷 metadata。
- `paper_questions`：试卷与题目的有序关联。
- `objects` / `object_blobs`：题面 TeX、答案 TeX、图片等统一对象存储。

关系上，题目先独立存在；试卷只是“按顺序引用题目”的容器。

## 核心 API

### 题目导入

- `POST /questions/imports/validate`
- `POST /questions/imports/commit`

这里导入的是一个“题目源目录”，其中只包含：

- `manifest.json`
- `questions/*.json`
- `latex/...`
- `assets/...`

### 试卷组装

- `POST /papers`
- `PUT /papers/{paper_id}/questions`

推荐流程是：

1. 先导入一批独立题目。
2. 再创建试卷 metadata。
3. 最后用有序 `question_refs` 绑定试卷包含哪些题。

### 查询与运维

- `GET /papers`
- `GET /papers/{paper_id}`
- `GET /questions`
- `GET /questions/{question_id}`
- `GET /search`
- `POST /exports/run`
- `POST /quality-checks/run`

## 样例生成

静态 `samples` 已经不再作为主要维护对象，推荐直接生成。

本仓库内置 [CPHOS-Latex](/home/be/Question_DB/CPHOS-Latex) 子项目，样例生成脚本会从它复制示例 TeX 和资源，生成一个可导入的题目源目录：

```bash
bash scripts/generate_samples.sh
```

默认输出到：

- [samples/generated](/home/be/Question_DB/samples/generated)

其中包含：

- `manifest.json`
- `questions/*.json`
- `latex/questions/*.tex`
- `assets/*`
- `api/create_paper.json`
- `api/replace_paper_questions.json`

后两个 JSON 可以直接被 `curl --data-binary @file` 用来创建试卷和设置题目顺序。

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

端到端测试会自动：

1. 生成样例
2. 启动 PostgreSQL
3. 启动 API
4. 调用 API 导入题目
5. 调用 API 创建试卷并绑定题目
6. 再查询和导出验证结果

```bash
bash scripts/test_full_flow.sh
```

## 数据库格式

表结构定义在 [0001_init_pg.sql](/home/be/Question_DB/migrations/0001_init_pg.sql)。

核心表：

- `objects` / `object_blobs`：统一对象元数据与二进制内容
- `questions`：独立题目主数据
- `question_assets`：题目资源
- `papers`：试卷元数据
- `paper_questions`：试卷与题目的有序关联
- `import_runs`：导入审计
