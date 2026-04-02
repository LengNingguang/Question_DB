// ============================================================
// 文件：src/api/questions/mod.rs
// 说明：题目管理模块入口
// ============================================================

//! 题目管理 API 模块
//!
//! 提供题目的 CRUD 操作、文件上传、批量下载等功能

// 声明子模块
pub(crate) mod handlers;   // HTTP 请求处理器
pub(crate) mod imports;    // ZIP 导入逻辑
pub(crate) mod models;     // 数据模型和验证
pub(crate) mod queries;    // 数据库查询构建

// 导入 Axum 路由类型
use axum::{routing::get, Router};

// 从 imports 模块重新导出上传大小限制常量
// 这样 api/mod.rs 可以直接使用 questions::MAX_UPLOAD_BYTES
pub(crate) use imports::MAX_UPLOAD_BYTES;

// ============================================================
// router 函数
// ============================================================
/// 创建题目管理模块的路由配置
///
/// # 返回
/// 配置好题目相关路由的 Router
///
/// # 路由表
/// ```text
/// /questions
/// ├── /              GET (list) / POST (create)
/// ├── /:question_id  GET (detail) / PATCH (update) / DELETE (delete)
/// └── /:question_id/file  PUT (replace)
/// ```
pub(crate) fn router() -> Router<super::AppState> {
    // 使用 Router::new() 创建新路由
    Router::new()
        // ====================================================
        // /questions 路由
        // ====================================================
        // 链式调用 .route() 添加多个端点
        // .route() 第一个参数是路径，第二个是处理器
        .route(
            "/questions",
            // GET /questions → 获取题目列表
            // POST /questions → 创建新题目（上传 ZIP）
            get(handlers::list_questions)
                .post(handlers::create_question),
        )
        // ====================================================
        // /questions/:question_id 路由
        // ====================================================
        // :question_id 是路径参数，可以通过 Path 提取器获取
        .route(
            "/questions/:question_id",
            // GET → 获取题目详情
            get(handlers::get_question_detail)
                // PATCH → 部分更新题目元数据
                .patch(handlers::update_question_metadata)
                // DELETE → 删除题目
                .delete(handlers::delete_question),
        )
        // ====================================================
        // /questions/:question_id/file 路由
        // ====================================================
        // PUT /questions/:id/file → 替换题目文件（重新上传 ZIP）
        .route(
            "/questions/:question_id/file",
            // 使用 axum::routing::put 显式指定 PUT 方法
            axum::routing::put(handlers::replace_question_file),
        )
}

/*
 * ============================================================
 * 知识点讲解 (Rust 新手必读)
 * ============================================================
 *
 * 1. 路径参数提取
 *    - /questions/:question_id 中的 :question_id 是占位符
 *    - Axum 会自动提取并传递给 handler
 *    - Handler 可以用 Path<T> 接收
 *
 * 2. 方法链式组合
 *    get(...).post(...).patch(...)
 *    同一路径可以注册多个 HTTP 方法
 *
 * 3. pub(crate) 可见性
 *    - pub(crate): 只在当前 crate 内公开
 *    - 外部 crate 无法访问
 *    - 适合内部实现细节
 *
 * 4. 路由分层组织
 *    - 每个功能领域有自己的 mod.rs
 *    - 主 mod.rs 通过 merge() 组合
 *    - 便于维护和测试
 *
 * ============================================================
 * 题目 API 端点一览
 * ============================================================
 *
 * GET /questions
 *   查询参数：category, tag, difficulty_tag, difficulty_min/max,
 *            q (搜索), paper_id, limit, offset
 *   返回：题目摘要列表
 *
 * POST /questions
 *   Content-Type: multipart/form-data
 *   字段：file (ZIP), description (文本), difficulty (JSON)
 *   返回：导入结果（question_id, 状态）
 *
 * GET /questions/:id
 *   返回：题目详情（含文件列表、关联试卷）
 *
 * PATCH /questions/:id
 *   Body: JSON (部分更新)
 *   可更新：category, description, tags, status, difficulty
 *
 * DELETE /questions/:id
 *   返回：删除结果
 *
 * PUT /questions/:id/file
 *   Content-Type: multipart/form-data
 *   字段：file (ZIP)
 *   返回：替换结果（新文件信息）
 *
 * POST /questions/bundles
 *   Body: JSON { question_ids: [...] }
 *   返回：ZIP 文件（含 manifest.json）
 *
 */
