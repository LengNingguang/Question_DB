# CPHOS 题库系统 - 代码分析报告

## 项目概述

这是一个用 Rust + Axum + PostgreSQL 构建的**物理竞赛题库管理系统**。

### 核心功能

1. **题目管理** - 上传、存储、查询物理竞赛题目
2. **试卷组装** - 将多道题目组合成试卷
3. **批量导出** - 导出题目包和试卷包（ZIP 格式）
4. **质量检查** - 审计数据完整性

### 技术栈

| 组件 | 技术 | 说明 |
|------|------|------|
| Web 框架 | Axum 0.7 | Rust 异步 Web 框架 |
| 数据库 | PostgreSQL | 关系型数据库 |
| ORM | SQLx 0.8 | 异步 SQL 工具库 |
| 序列化 | Serde | JSON 序列化/反序列化 |
| 压缩 | Zip 2 | ZIP 文件处理 |
| 日志 | Tracing | 结构化日志 |

---

## 项目架构

```
src/
├── api/                      # HTTP API 层
│   ├── mod.rs               # API 模块入口，路由组合
│   ├── shared/              # 共享工具
│   │   ├── error.rs         # 统一错误处理
│   │   └── utils.rs         # 工具函数
│   ├── system/              # 系统接口
│   │   └── handlers.rs      # 健康检查
│   ├── questions/           # 题目管理
│   │   ├── handlers.rs      # HTTP 请求处理
│   │   ├── models.rs        # 数据模型
│   │   ├── queries.rs       # 数据库查询构建
│   │   └── imports.rs       # ZIP 导入逻辑
│   ├── papers/              # 试卷管理
│   │   ├── handlers.rs      # HTTP 请求处理
│   │   ├── models.rs        # 数据模型
│   │   ├── queries.rs       # 数据库查询构建
│   │   └── imports.rs       # ZIP 导入逻辑
│   └── ops/                 # 运维操作
│       ├── bundles.rs       # 批量打包
│       ├── exports.rs       # 数据导出
│       ├── paper_render.rs  # LaTeX 渲染
│       └── quality.rs       # 质量检查
├── config.rs                 # 配置管理
├── db.rs                     # 数据库连接池
├── lib.rs                    # 库入口
└── main.rs                   # 程序入口
```

---

## 数据模型

### 数据库表结构

```sql
objects              -- 文件存储表（题目源文件、资源文件）
├── object_id        -- UUID 主键
├── file_name        -- 文件名
├── mime_type        -- MIME 类型
├── size_bytes       -- 文件大小
├── content          -- 二进制内容
└── created_at       -- 创建时间

questions            -- 题目元数据表
├── question_id      -- UUID 主键
├── source_tex_path  -- 源 TeX 文件路径
├── category         -- 分类 (none/T=理论/E=实验)
├── status           -- 状态 (none/reviewed/used)
├── description      -- 描述
└── created_at/updated_at

question_files       -- 题目文件关联表
├── question_id      -- 关联题目
├── object_id        -- 关联文件
├── file_kind        -- 类型 (tex/asset)
└── file_path        -- 文件路径

question_tags        -- 题目标签表
├── question_id      -- 关联题目
├── tag              -- 标签名
└── sort_order       -- 排序

question_difficulties -- 题目难度表
├── question_id      -- 关联题目
├── algorithm_tag    -- 算法标签 (human/ml/heuristic)
├── score            -- 分数 (1-10)
└── notes            -- 备注

papers               -- 试卷表
├── paper_id         -- UUID 主键
├── description      -- 描述
├── title            -- 标题
├── subtitle         -- 副标题
├── authors          -- 命题人数组
├── reviewers        -- 审题人数组
└── append_object_id -- 附加文件 ID

paper_questions      -- 试卷题目关联表
├── paper_id         -- 关联试卷
├── question_id      -- 关联题目
└── sort_order       -- 题目顺序
```

---

## API 端点一览

### 系统接口
- `GET /health` - 健康检查

### 题目接口
- `GET /questions` - 查询题目列表（支持多种过滤条件）
- `POST /questions` - 上传新题目（ZIP 格式）
- `GET /questions/{id}` - 获取题目详情
- `PATCH /questions/{id}` - 更新题目元数据
- `DELETE /questions/{id}` - 删除题目
- `PUT /questions/{id}/file` - 替换题目文件
- `POST /questions/bundles` - 批量下载题目包

### 试卷接口
- `GET /papers` - 查询试卷列表
- `POST /papers` - 创建新试卷
- `GET /papers/{id}` - 获取试卷详情
- `PATCH /papers/{id}` - 更新试卷
- `DELETE /papers/{id}` - 删除试卷
- `PUT /papers/{id}/file` - 替换试卷附加文件
- `POST /papers/bundles` - 批量下载试卷包

### 运维接口
- `POST /exports/run` - 导出数据（JSONL/CSV）
- `POST /quality-checks/run` - 运行质量检查

---

## 核心流程详解

### 1. 题目上传流程

```
用户上传 ZIP → 解析 ZIP → 验证结构 → 存入数据库
                    ↓
            标准结构:
            - problem.tex (必须)
            - assets/ (必须)
```

### 2. 试卷创建流程

```
用户上传 ZIP(附加材料) + 题目 ID 列表 → 验证题目 → 创建试卷记录
                                              ↓
                                      验证规则:
                                      - 所有题目必须同类别 (T 或 E)
                                      - 所有题目必须已审核
```

### 3. 试卷渲染流程

```
加载试卷 → 加载题目 → 提取 problem 环境 → 
       ↓                      ↓
   LaTeX 模板            重命名资源路径
       ↓
   注入元数据 (标题、作者等)
       ↓
   生成完整 LaTeX 文档
```

### 4. 批量打包流程

```
题目/试卷 ID 列表 → 加载数据 → 生成 manifest.json → 
                              ↓
                        包含:
                        - 元数据
                        - 文件映射
                        - 关联关系
                              ↓
                     写入 ZIP 返回用户
```

---

## 关键设计决策

### 1. ZIP 格式规范

**题目 ZIP 要求:**
- 根目录必须有且仅有一个 `.tex` 文件
- 根目录必须有且仅有一个 `assets/` 目录
- 不允许其他文件或目录

**原因:** 简化验证逻辑，统一存储格式

### 2. 难度系统设计

```json
{
  "human": {"score": 5, "notes": "人工标定基准"},
  "ml": {"score": 7, "notes": "AI 模型评估"},
  "heuristic": {"score": 6}
}
```

- 支持多种评估算法同时存在
- `human` 是必填项
- 分数范围 1-10

### 3. 试卷验证规则

- **类别一致性**: 所有题目必须同为理论 (T) 或实验 (E)
- **状态要求**: 所有题目必须是 `reviewed` 或 `used`
- **顺序保留**: `question_ids` 数组顺序决定题目顺序

### 4. LaTeX 渲染机制

使用模板替换方式：
1. 加载预定义的 LaTeX 模板
2. 替换标题、作者等元数据
3. 提取每道题的 `problem` 环境
4. 重命名资源路径避免冲突
5. 添加前缀区分同名 label

---

## 安全与验证

### 输入验证
- ZIP 大小限制 20MB
- 解压后大小限制 64MB
- 路径遍历防护（禁止 `../`）
- 文件名安全检查（禁止特殊字符）

### 错误处理
- 统一 `ApiError` 类型
- 400: 客户端错误（验证失败）
- 404: 资源不存在
- 500: 服务器内部错误

---

## 测试策略

### 单元测试
- 配置解析测试
- 模型验证测试
- ZIP 解析测试
- LaTeX 渲染测试

### 端到端测试 (Python)
- 完整 CRUD 流程
- 真实数据测试（test.zip, test2.zip）
- 边界条件测试
- 错误场景测试

---

## 分级阅读指南

### 入门级 (Rust 新手)
1. `src/main.rs` - 程序入口，理解 Rust 异步 main
2. `src/lib.rs` - 模块导出
3. `src/config.rs` - 环境变量读取
4. `src/db.rs` - 数据库连接池
5. `src/api/shared/error.rs` - 错误处理模式

### 进阶级 (理解 Web API)
1. `src/api/system/handlers.rs` - 最简单的健康检查接口
2. `src/api/questions/mod.rs` - 路由定义
3. `src/api/questions/models.rs` - 请求/响应模型
4. `src/api/questions/handlers.rs` - 请求处理逻辑

### 高级级 (深入业务逻辑)
1. `src/api/questions/imports.rs` - ZIP 解析和导入
2. `src/api/ops/bundles.rs` - 批量打包逻辑
3. `src/api/ops/paper_render.rs` - LaTeX 模板渲染
4. `src/api/papers/handlers.rs` - 试卷组装验证

### 专家级 (完整系统理解)
1. `scripts/tests/full_flow.py` - 端到端测试流程
2. `migrations/0001_init_pg.sql` - 数据库设计
3. `Cargo.toml` - 依赖管理和版本选择

---

## 学习建议

### 给 Rust 新手的建议

1. **先理解类型系统**
   - `struct` 定义数据结构
   - `impl` 实现方法
   - `trait` 定义行为接口
   - `Result<T, E>` 错误处理

2. **理解异步编程**
   - `async/await` 语法
   - `Tokio` 运行时
   - `?` 操作符传播错误

3. **理解所有权**
   - `&T` 不可变引用
   - `&mut T` 可变引用
   - 生命周期 `'a`

4. **推荐学习路径**
   - 从 `main.rs` 开始理解程序启动
   - 阅读 `config.rs` 理解错误处理
   - 阅读 `handlers.rs` 理解 HTTP 处理
   - 最后阅读 `imports.rs` 理解复杂业务
