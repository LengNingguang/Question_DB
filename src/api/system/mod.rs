// ============================================================
// 文件：src/api/system/mod.rs
// 说明：系统接口模块入口
// ============================================================

// 声明 handlers 子模块，包含具体的请求处理函数
mod handlers;

// 导入 Axum 的路由构建器
use axum::{routing::get, Router};

// ============================================================
// router 函数
// ============================================================
/// 创建系统接口的路由配置
///
/// # 返回
/// 配置好路由的 Router 对象
///
/// # 路由表
/// - GET /health → handlers::health
pub(crate) fn router() -> Router<super::AppState> {
    // Router::new() 创建新路由
    // .route() 添加路由规则
    // get() 指定 HTTP GET 方法
    Router::new()
        .route("/health", get(handlers::health))
}

/*
 * ============================================================
 * 知识点：Axum 路由
 * ============================================================
 *
 * Router 是 Axum 的核心类型，用于定义 URL 到处理器的映射
 *
 * 基本用法:
 * Router::new()
 *     .route("/path", get(handler))      // GET 请求
 *     .route("/path", post(handler))     // POST 请求
 *     .route("/path", put(handler))      // PUT 请求
 *     .route("/path", delete(handler))   // DELETE 请求
 *     .route("/path", patch(handler))    // PATCH 请求
 *
 * 路由合并:
 * router1.merge(router2) 合并两个路由
 *
 * 状态传递:
 * .with_state(state) 将状态传递给所有 handler
 *
 */
