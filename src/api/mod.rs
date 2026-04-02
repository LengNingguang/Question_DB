// ============================================================
// 文件：src/api/mod.rs
// 说明：HTTP API 层的主入口，组合所有子模块路由
// ============================================================

//! HTTP API 组合模块
//!
//! 这个文件是整个 API 层的入口，负责：
//! 1. 声明所有子模块
//! 2. 导出公共类型
//! 3. 组合完整的路由树

// 声明子模块
// 每个子模块对应一个功能领域
mod ops;         // 运维操作（打包、导出、质量检查）
mod papers;      // 试卷管理
mod questions;   // 题目管理
mod shared;      // 共享工具和错误处理
mod system;      // 系统接口（健康检查）
mod tests;       // 集成测试

// 导入 Axum 路由类型
use axum::{extract::DefaultBodyLimit, Router};

// 导入 SQLx 数据库连接池类型
use sqlx::PgPool;

// ============================================================
// 公共类型导出
// ============================================================
// 这些类型被 main.rs 或其他外部模块使用
// 通过 pub use 重新导出，简化调用方的导入路径

// 从 papers 模块导出试卷相关类型
pub use self::papers::models::{
    PaperDetail,      // 试卷详情（包含题目列表）
    PaperQuestionSummary,  // 试卷中题目的摘要
    PaperSummary,     // 试卷摘要（不含题目详情）
};

// 从 questions 模块导出题目相关类型
pub use self::questions::models::{
    QuestionAssetRef,  // 题目资源文件引用
    QuestionDetail,    // 题目详情（完整信息）
    QuestionPaperRef,  // 题目所属试卷引用
    QuestionSummary,   // 题目摘要
};

// ============================================================
// AppState 结构体
// ============================================================
/// 应用共享状态
///
/// 通过 Axum 的 State 提取器在所有请求处理器间共享
///
/// # 字段
/// - pool: PostgreSQL 连接池，所有数据库操作都通过它
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
}

// ============================================================
// router 函数
// ============================================================
/// 构建完整的 HTTP 应用路由
///
/// # 参数
/// - state: 应用共享状态（数据库连接池）
///
/// # 返回
/// 配置完整的 Axum Router，可以直接用于启动服务器
///
/// # 路由结构
/// ```text
/// /
/// ├── health              (system)
/// ├── questions           (questions)
/// │   ├── /               GET/POST
/// │   ├── /:id            GET/PATCH/DELETE
/// │   ├── /:id/file       PUT
/// │   └── /bundles        POST
/// ├── papers              (papers)
/// │   ├── /               GET/POST
/// │   ├── /:id            GET/PATCH/DELETE
/// │   ├── /:id/file       PUT
/// │   └── /bundles        POST
/// ├── exports             (ops)
/// │   └── /run            POST
/// └── quality-checks      (ops)
///     └── /run            POST
/// ```
pub fn router(state: AppState) -> Router {
    // 使用 Router::new() 创建空路由
    Router::new()
        // ====================================================
        // 合并子路由
        // ====================================================
        // .merge() 将子模块的路由合并到主路由
        // 合并顺序不影响匹配优先级（Axum 按路径精确度匹配）
        .merge(system::router())    // /health
        .merge(papers::router())    // /papers/*
        .merge(questions::router()) // /questions/*
        .merge(ops::router())       // /exports/*, /quality-checks/*

        // ====================================================
        // 添加请求体大小限制层
        // ====================================================
        // DefaultBodyLimit 是 Axum 的中间件层
        // 限制上传请求体的最大大小
        // max() 参数取题目和试卷上传限制的最大值
        .layer(DefaultBodyLimit::max(
            questions::MAX_UPLOAD_BYTES.max(papers::MAX_UPLOAD_BYTES),
        ))

        // ====================================================
        // 注入应用状态
        // ====================================================
        // .with_state() 将状态传递给所有路由处理器
        // 处理器可以通过 State<AppState> 提取器访问
        .with_state(state)
}

/*
 * ============================================================
 * 知识点讲解 (Rust 新手必读)
 * ============================================================
 *
 * 1. 模块可见性
 *    - mod: 私有模块，仅当前文件可访问
 *    - pub mod: 公共模块，其他模块可通过路径访问
 *    - pub use: 重新导出，简化调用方导入
 *
 * 2. Router 组合模式
 *    - Router::new() 创建空路由
 *    - .route() 添加单个路由
 *    - .merge() 合并子路由
 *    - .layer() 添加中间件
 *    - .with_state() 注入状态
 *
 * 3. Clone trait
 *    - AppState 需要 Clone 因为每个请求都要复制状态
 *    - PgPool 内部使用 Arc，clone 开销很小
 *
 * 4. 中间件层 (Layer)
 *    - Layer 是 Tower 库的核心概念
 *    - 可以在请求处理前后执行逻辑
 *    - 常见用途：日志、认证、限流、CORS
 *
 * ============================================================
 * 路由注册流程图
 * ============================================================
 *
 * router() 调用
 *     ↓
 * Router::new() 创建空路由
 *     ↓
 * .merge(system::router())    添加 /health
 *     ↓
 * .merge(papers::router())    添加 /papers/*
 *     ↓
 * .merge(questions::router()) 添加 /questions/*
 *     ↓
 * .merge(ops::router())       添加 /exports/*, /quality-checks/*
 *     ↓
 * .layer(DefaultBodyLimit)    添加大小限制
 *     ↓
 * .with_state(state)          注入数据库连接池
 *     ↓
 * 返回完整 Router
 *
 */
