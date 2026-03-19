# CPHOS Question Bank

Rust + Axum + PostgreSQL 题库服务，功能：题库导入、查询、统计、难度计算、导出、质量检查。

## 目录结构
- `src/`: API 与核心业务
- `migrations/`: PostgreSQL schema
- `tests/`: Rust 集成测试
- `scripts/test_full_flow.sh`: Docker + PostgreSQL 端到端测试
- `samples/demo_bundle/`: 演示用 bundle
- `assets/demo/`: 演示 bundle 依赖图片

## 核心接口
- `POST /imports/bundle/validate`
- `POST /imports/bundle/commit`
- `POST /imports/workbooks/commit`
- `POST /imports/stats/commit`
- `POST /difficulty-scores/run`
- `GET /papers`
- `GET /questions`
- `GET /search`
- `GET /score-workbooks/{workbook_id}/download`
- `POST /exports/run`
- `POST /quality-checks/run`

## 启动
```bash
export QB_DATABASE_URL='postgres://postgres:postgres@127.0.0.1:5432/qb'
export QB_BIND_ADDR='127.0.0.1:8080'
cargo run
```

初始化数据库：
```bash
psql "$QB_DATABASE_URL" -f migrations/0001_init_pg.sql
```

## 测试
单元与集成测试：
```bash
cargo test
```

端到端测试：
```bash
bash scripts/test_full_flow.sh
```

## 数据库格式
表结构定义在 [0001_init_pg.sql](/home/be/Question_DB/migrations/0001_init_pg.sql)。

核心表：
- `objects` / `object_blobs`: 统一对象元数据与二进制内容
- `papers`: 试卷
- `questions`: 题目
- `question_assets`: 题目资源
- `score_workbooks`: 原始 workbook
- `question_stats`: 按题统计
- `difficulty_scores`: 难度结果
- `import_runs`: 导入审计
