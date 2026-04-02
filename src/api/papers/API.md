# Papers API

## Endpoints

### `POST /papers`

创建试卷，并按 `question_ids` 的顺序写入题目关联。

请求格式：`multipart/form-data`

字段说明：

- `file`: 必填，试卷附加 zip 文件；服务端会校验它是合法 zip，但暂时不检查内部结构
- `description`: 必填，非空字符串；会参与 bundle 目录命名，因此不能包含 `/ \\ : * ? " < > |`，不能是 `.`、`..`，也不能以 `.` 结尾
- `title`: 必填，非空字符串
- `subtitle`: 必填，非空字符串
- `authors`: 必填，JSON 字符串数组，例如 `["Alice","Bob"]`
- `reviewers`: 必填，JSON 字符串数组，例如 `["Carol"]`
- `question_ids`: 必填，JSON 字符串数组，例如 `["uuid-1","uuid-2"]`
  - 这些题目必须全部属于同一类：要么全部是 `T`，要么全部是 `E`
  - 每道题的 `status` 必须是 `reviewed` 或 `used`

示例：

```bash
curl -X POST http://127.0.0.1:8080/papers \
  -F 'description=综合训练试卷 A' \
  -F 'title=综合训练 2026 A 卷' \
  -F 'subtitle=校内选拔 初版' \
  -F 'authors=["Alice Zhang","Bob Chen"]' \
  -F 'reviewers=["Carol Xu"]' \
  -F 'question_ids=["uuid-1","uuid-2"]' \
  -F 'file=@paper_appendix.zip;type=application/zip'
```

### `GET /papers`

按条件分页查询试卷，搜索也统一走这个接口。

支持的 query 参数：

- `question_id`
- `category`
- `tag`
- `q`
  关键词会模糊匹配 `description`、`title`、`subtitle`、`authors`、`reviewers`
- `limit`
- `offset`

### `GET /papers/{paper_id}`

返回试卷详情和按顺序展开后的题目摘要。

### `PATCH /papers/{paper_id}`

部分更新试卷 metadata 和题目列表。

支持字段：

- `description`
- `title`
- `subtitle`
- `authors`
- `reviewers`
- `question_ids`

其中：

- `description` 如果出现在更新请求里，必须是非空字符串，并且同样要满足文件名安全限制
- `title` / `subtitle` 如果出现在更新请求里，必须是非空字符串
- `authors` / `reviewers` 如果出现在更新请求里，必须是字符串数组
- `question_ids` 如果出现在更新请求里，必须是非空 UUID 字符串数组；成功后会按数组顺序重排题目
- 更新请求会校验试卷更新后的整套题目：
  - `category` 必须全部同为 `T` 或全部同为 `E`
  - 每道题的 `status` 必须是 `reviewed` 或 `used`

成功时返回更新后的完整试卷详情。

### `PUT /papers/{paper_id}/file`

使用 `multipart/form-data` 覆盖试卷当前的附加 zip 文件，只更新文件，不修改 metadata 或题目列表。

- 字段名：`file`
- 必须是合法 zip
- 大小限制：20 MiB
- 成功后会：
  - 新写入一个 appendix object
  - 更新 `append_object_id`
  - 删除旧的 appendix object
  - 更新 `updated_at`

成功响应：

```json
{
  "paper_id": "uuid",
  "file_name": "paper_appendix_v2.zip",
  "status": "replaced"
}
```

### `DELETE /papers/{paper_id}`

删除试卷。

成功响应：

```json
{
  "paper_id": "uuid",
  "status": "deleted"
}
```

### `POST /papers/bundles`

按给定试卷列表批量打包下载。

请求体：

```json
{
  "paper_ids": ["uuid-1", "uuid-2"]
}
```

返回值：

- 响应体是一个 `application/zip`
- zip 根目录包含 `manifest.json`
- 每个试卷使用 `description_uuid前缀/` 目录分组，例如 `热学决赛卷_550e84/`
- 每个试卷目录下包含：
  - `append.zip`
  - `main.tex`
  - 单个合并后的 `assets/` 目录
- `main.tex` 基于内置的 `CPHOS-Latex` 理论/实验 `example-paper.tex` 模板生成
- 题目会按试卷中的顺序依次注入 `main.tex`
- 每道题原始 tex 中的 `\includegraphics` 资源引用会被改写到合并后的 `assets/` 目录
- 每道题内部的 `\label` / `\ref` / `\eqref` 等标签会按 `p1-`、`p2-` 这样的前缀重写，避免跨题冲突
