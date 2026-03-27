# Questions API

## Endpoints

### `POST /questions`

使用 `multipart/form-data` 上传单题 zip 压缩包。

- 字段名：`file`
- 必填字段：`description`
  - 必须是非空字符串
  - 支持中文
  - 不能包含 `/ \\ : * ? " < > |`
  - 不能是 `.`、`..`，也不能以 `.` 结尾
- 必填字段：`difficulty`
  - 传 JSON 字符串
  - 必须至少包含 `human`
  - 每个 tag 的值形如 `{ "score": 7, "notes": "sample" }`
- 大小限制：20 MiB
- zip 根目录必须包含且只包含：
  - 恰好一个 `.tex` 文件
  - 恰好一个 `assets/` 目录
- 上传时自动写入默认 metadata：
  - `category = "none"`
  - `tags = []`
  - `status = "none"`
  - `created_at = NOW()`

成功响应：

```json
{
  "question_id": "uuid",
  "file_name": "question.zip",
  "imported_assets": 2,
  "status": "imported"
}
```

### `PATCH /questions/{question_id}`

使用 JSON 请求体更新题目的 metadata，支持部分更新。

支持字段：

- `category`: `none` | `T` | `E`
- `description`: 非空字符串，不能传 `null` 或空串
  - 同样不能包含 `/ \\ : * ? " < > |`
- `tags`: 字符串数组，会去重；空数组表示清空
- `status`: `none` | `reviewed` | `used`
- `difficulty`: 对象
  - key 是 difficulty tag，例如 `human`、`heuristic`
  - value 形如 `{ "score": 7, "notes": "sample" }`
  - `score` 必须是 `1..=10`
  - `notes` 可选；空串会规范化为 `null`
  - 如果传了 `difficulty`，会整体替换整组 difficulty
  - `difficulty` 必须至少包含 `human`

成功时返回更新后的完整题目详情。

### `PUT /questions/{question_id}/file`

使用 `multipart/form-data` 覆盖题目的当前 zip 文件内容，只更新文件，不修改 metadata。

- 字段名：`file`
- 大小限制：20 MiB
- zip 根目录必须包含且只包含：
  - 恰好一个 `.tex` 文件
  - 恰好一个 `assets/` 目录
- 成功后会：
  - 删除题目当前关联的 tex / asset 文件对象
  - 写入新 zip 中的 tex / asset 文件
  - 更新 `source_tex_path`
  - 更新 `updated_at`
- 原有 metadata 会保留：
  - `category`
  - `description`
  - `tags`
  - `status`
  - `difficulty`

成功响应：

```json
{
  "question_id": "uuid",
  "file_name": "question_v2.zip",
  "source_tex_path": "main.tex",
  "imported_assets": 3,
  "status": "replaced"
}
```

### `DELETE /questions/{question_id}`

删除题目。

成功响应：

```json
{
  "question_id": "uuid",
  "status": "deleted"
}
```

### `GET /questions`

按条件分页查询题目，搜索也统一走这个接口。

支持的 query 参数：

- `paper_id`
- `category`
- `tag`
- `difficulty_tag`
- `difficulty_min`
- `difficulty_max`
- `q`
  关键词搜索，只会匹配 `description`
- `limit`
- `offset`

说明：

- `difficulty_min` / `difficulty_max` 需要和 `difficulty_tag` 一起使用
- difficulty 过滤会匹配指定 tag 上的 score 范围

### `GET /questions/{question_id}`

返回单个题目的完整 metadata、文件引用和所属试卷。

### `POST /questions/bundles`

按给定题目列表批量打包下载。

请求体：

```json
{
  "question_ids": ["uuid-1", "uuid-2", "uuid-3"]
}
```

返回值：

- 响应体是一个 `application/zip`
- zip 根目录包含 `manifest.json`
- 每个题目使用 `description_uuid前缀/` 目录分组，例如 `热学标定 gamma_550e84/`
- 目录内包含原始 `.tex` 和 `assets/` 资源文件
