# Ops API

## Endpoints

### `POST /exports/run`

导出题目数据。

请求体：

```json
{
  "format": "jsonl",
  "public": false,
  "output_path": "exports/question_bank_internal.jsonl"
}
```

### `POST /quality-checks/run`

运行数据质量检查，并把结果写到指定文件。

请求体：

```json
{
  "output_path": "exports/quality_report.json"
}
```
