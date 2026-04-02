// ============================================================
// 文件：tests/health_route.rs
// 说明：健康检查路由集成测试
// ============================================================

//! 测试 /health 端点在数据库不可用时的行为
//!
//! 验证当数据库连接失败时，健康检查返回 503 服务不可用

// 导入 Axum Web 框架类型
use axum::{
    body::Body,
    http::{Request, StatusCode},  // 请求和状态码
};

// 导入 HTTP Body 工具
use http_body_util::BodyExt;

// 导入被测 API
use qb_api::api::{router, AppState};

// 导入 SQLx PostgreSQL 连接池
use sqlx::postgres::PgPoolOptions;

// 导入 Tower 服务工具
use tower::ServiceExt;

// ============================================================
// 健康路由测试
// ============================================================
/// 测试：当数据库不可达时，/health 返回 503
///
/// # 测试逻辑
/// 1. 创建指向无效地址的数据库连接池
/// 2. 构建 API 路由
/// 3. 发送 GET /health 请求
/// 4. 验证响应状态为 503 SERVICE_UNAVAILABLE
/// 5. 验证响应体为空
#[tokio::test]
async fn health_route_returns_service_unavailable_when_db_is_unreachable() {
    // 步骤 1: 创建数据库连接池
    // 指向无效地址 127.0.0.1:1 (不可能有服务)
    let pool = PgPoolOptions::new()
        // 设置极短的获取超时（50ms），加速测试
        .acquire_timeout(std::time::Duration::from_millis(50))
        // 最大连接数设为 1（测试不需要更多）
        .max_connections(1)
        // 连接到无效地址（懒连接，实际使用时才会尝试）
        .connect_lazy("postgres://postgres:postgres@127.0.0.1:1/qb")
        .unwrap();

    // 步骤 2: 构建 API 路由
    // AppState 包含数据库连接池
    let app = router(AppState { pool });

    // 步骤 3: 发送 GET /health 请求
    // 使用 oneshot 直接调用服务（不需要启动 HTTP 服务器）
    let response = app
        .oneshot(
            // 构建请求
            Request::builder()
                .uri("/health")  // 请求路径
                .body(Body::empty())  // 空请求体
                .unwrap(),
        )
        .await
        .unwrap();

    // 步骤 4: 验证响应状态
    // 期望：503 SERVICE_UNAVAILABLE
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

    // 步骤 5: 读取并验证响应体
    // 将响应体收集为字节
    let body = response.into_body().collect().await.unwrap().to_bytes();
    // 验证响应体为空（503 不需要返回错误详情）
    assert!(body.is_empty());
}

/*
 * ============================================================
 * 知识点讲解 (Rust 新手必读)
 * ============================================================
 *
 * 1. #[tokio::test] 属性
 *    - 标记异步测试函数
 *    - tokio 运行时执行测试
 *    - 允许使用 .await
 *
 * 2. connect_lazy()
 *    - 创建"懒"连接池
 *    - 不立即建立连接
 *    - 第一次使用时才尝试连接
 *    - 适合测试场景
 *
 * 3. ServiceExt::oneshot()
 *    - 直接调用服务处理器
 *    - 不需要启动 HTTP 服务器
 *    - 测试更快速、更隔离
 *
 * 4. Request 构建器模式
 *    Request::builder()
 *        .uri("/health")
 *        .body(Body::empty())
 *        .unwrap()
 *
 * 5. BodyExt::collect()
 *    - 收集流式响应体
 *    - 返回 Bytes 类型
 *    - 需要 .await
 *
 * ============================================================
 * 测试设计思路
 * ============================================================
 *
 * 测试目标:
 *   验证数据库连接失败时的错误处理
 *
 * 为什么有效:
 *   1. 使用 127.0.0.1:1 - 这个端口几乎不可能有服务
 *   2. 50ms 超时 - 测试不会等待太久
 *   3. 懒连接 - 创建时不失败，请求时才失败
 *
 * 预期行为:
 *   /health 端点尝试获取数据库连接
 *   → 连接失败（超时或拒绝）
 *   → 返回 503 SERVICE_UNAVAILABLE
 *   → 响应体为空（不泄露内部错误）
 *
 * ============================================================
 * 健康检查 API 设计
 * ============================================================
 *
 * 成功情况 (200 OK):
 *   - 数据库连接正常
 *   - 所有依赖服务可用
 *   - 返回：空响应体或简单 JSON
 *
 * 失败情况 (503 SERVICE_UNAVAILABLE):
 *   - 数据库连接失败
 *   - 依赖服务不可用
 *   - 返回：空响应体
 *
 * 为什么返回空响应体:
 *   - 减少信息泄露
 *   - 调用方只关心状态码
 *   - 详细错误记录在服务器日志
 *
 * ============================================================
 * 测试代码流程
 * ============================================================
 *
 * 1. 创建连接池
 *    └─> PgPoolOptions::new()
 *    └─> acquire_timeout(50ms)
 *    └─> max_connections(1)
 *    └─> connect_lazy("invalid-url")
 *
 * 2. 构建路由
 *    └─> router(AppState { pool })
 *
 * 3. 发送请求
 *    └─> Request::builder().uri("/health")...
 *    └─> app.oneshot(request).await
 *
 * 4. 验证响应
 *    └─> assert_eq!(status, 503)
 *    └─> assert!(body.is_empty())
 *
 */
