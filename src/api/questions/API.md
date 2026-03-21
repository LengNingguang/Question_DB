# Questions API

## Endpoints

### `POST /questions`

使用 `multipart/form-data` 上传单题 zip 压缩包。

- 字段名：`file`
- 大小限制：20 MiB
- zip 根目录必须包含且只包含：
  - 恰好一个 `.tex` 文件
  - 恰好一个 `assets/` 目录
- 上传时自动写入默认 metadata：
  - `category = "none"`
  - `notes = ""`
  - `tags = []`
  - `status = "none"`
  - `difficulty = {}`
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
- `notes`: 字符串，传 `null` 或空串会清空为 `""`
- `tags`: 字符串数组，会去重；空数组表示清空
- `status`: `none` | `reviewed` | `used`
- `difficulty`: 对象
  - `human`: `1..=10`，传 `null` 清空
  - `algorithm`: `{ "algo": 7 }`，传空对象清空全部算法分数
  - `notes`: 字符串，传 `null` 或空串清空
  - 传 `{}` 会清空整个 `difficulty`

成功时返回更新后的完整题目详情。

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
- `paper_type`
- `category`
- `tag`
- `q`
  关键词搜索，会匹配 `question_id`、`notes` 和 `source.tex`
- `limit`
- `offset`

### `GET /questions/{question_id}`

返回单个题目的完整 metadata、文件引用和所属试卷。
