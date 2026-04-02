// ============================================================
// 文件：src/api/shared/error.rs
// 说明：统一的 API 错误处理
// ============================================================

//! 共享 API 错误类型和 JSON 错误响应
//!
//! 定义了整个 API 层使用的错误处理机制
//! 确保所有接口返回统一的错误格式

// 导入 Axum Web 框架的相关类型
use axum::{
    http::StatusCode,  // HTTP 状态码 (200, 404, 500 等)
    response::{IntoResponse, Response},  // 响应转换 trait
    Json,  // JSON 响应类型
};

// 导入 Serde 序列化库
// Serialize trait 用于将 Rust 类型转换为 JSON
use serde::Serialize;

// 导入 serde_json 的 json! 宏
// 用于方便地创建 JSON 对象
use serde_json::json;

// ============================================================
// ApiError 结构体
// ============================================================
// 定义 API 错误的内部表示
// pub(crate) 表示只在当前 crate 内可见
#[derive(Debug)]
pub(crate) struct ApiError {
    /// HTTP 响应状态码
    pub(crate) status: StatusCode,

    /// 错误消息 (返回给客户端)
    pub(crate) message: String,
}

// ============================================================
// 类型别名：ApiResult
// ============================================================
// Result<T, ApiError> 的简化写法
// 用于 API 处理函数的返回类型
// Json<T> 表示成功时返回 JSON 格式的数据
pub(crate) type ApiResult<T> = Result<Json<T>, ApiError>;

// ============================================================
// ApiError 的实现
// ============================================================
impl ApiError {
    /// 创建 400 Bad Request 错误
    ///
    /// # 参数
    /// message: 错误描述消息
    pub(crate) fn bad_request(message: impl Into<String>) -> Self {
        Self {
            // 400 状态码表示客户端请求有误
            status: StatusCode::BAD_REQUEST,
            // .into() 将参数转换为 String
            message: message.into(),
        }
    }

    /// 创建 500 Internal Server Error 错误
    ///
    /// # 参数
    /// message: 错误描述消息
    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self {
            // 500 状态码表示服务器内部错误
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

// ============================================================
// IntoResponse trait 实现
// ============================================================
// 将 ApiError 转换为 Axum 的 HTTP 响应
// 这样 ApiError 可以直接从 handler 函数返回
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        // 创建 JSON 响应体
        // json! 宏创建如下格式的 JSON:
        // {"error": "错误消息"}
        let payload = Json(json!({ "error": self.message }));

        // 返回 (状态码，JSON 体) 元组
        // Axum 会自动将元组转换为完整响应
        (self.status, payload).into_response()
    }
}

// ============================================================
// From<anyhow::Error> trait 实现
// ============================================================
// 允许使用 ? 操作符将 anyhow::Error 转换为 ApiError
// 简化错误传播
impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        // 将 anyhow 错误转换为 500 内部错误
        // 使用 err.to_string() 获取错误消息
        Self::internal(err.to_string())
    }
}

// ============================================================
// HealthResponse 结构体
// ============================================================
// 健康检查接口的响应格式
#[derive(Debug, Serialize)]
pub(crate) struct HealthResponse {
    /// 服务状态 ("ok" 或 "error")
    pub(crate) status: &'static str,

    /// 服务名称
    pub(crate) service: &'static str,
}

/*
 * ============================================================
 * 知识点讲解 (Rust 新手必读)
 * ============================================================
 *
 * 1. 结构体字段可见性
 *    - pub(crate): 只在当前 crate 内可见
 *    - pub: 完全公开
 *    - 默认 (无修饰): 私有
 *
 * 2. impl Into<String>
 *    - 允许函数接受 String 或 &str
 *    - .into() 自动转换为 String
 *    - 提高 API 灵活性
 *
 * 3. trait 实现
 *    - IntoResponse: 定义如何转换为 HTTP 响应
 *    - From: 定义类型转换逻辑
 *    - trait 是 Rust 实现多态的方式
 *
 * 4. json! 宏
 *    - 方便地创建 JSON 对象
 *    - 支持插值：json!({"key": $value})
 *    - 返回 serde_json::Value 类型
 *
 * 5. &'static str
 *    - 生命周期为 'static 的字符串引用
 *    - 通常用于字符串字面量
 *    - 在程序整个生命周期内有效
 *
 * ============================================================
 * 错误处理流程图
 * ============================================================
 *
 * handler 函数
 *     ↓
 * 发生错误 (anyhow::Error)
 *     ↓
 * ? 操作符传播错误
 *     ↓
 * From<anyhow::Error> 转换为 ApiError
 *     ↓
 * IntoResponse::into_response()
 *     ↓
 * 返回 HTTP 响应 (状态码 + JSON)
 *
 * 响应示例:
 * HTTP/1.1 400 Bad Request
 * Content-Type: application/json
 * {"error": "无效的请求参数"}
 *
 */
