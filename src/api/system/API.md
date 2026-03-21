# System API

## Endpoints

### `GET /health`

健康检查接口。会执行一次数据库连通性探测：

- 成功时返回 `200`
- 数据库不可达时返回 `503`
