// ============================================================
// 文件：src/api/papers/mod.rs
// 说明：试卷管理模块入口
// ============================================================

//! 试卷管理 API 模块
//!
//! 提供试卷的 CRUD 操作、题目组装、批量下载等功能

// 声明子模块
pub(crate) mod handlers;   // HTTP 请求处理器
pub(crate) mod imports;    // ZIP 导入逻辑
pub(crate) mod models;     // 数据模型和验证
pub(crate) mod queries;    // 数据库查询构建

// 导入 Axum 路由类型
use axum::{routing::get, Router};

// 从 imports 模块重新导出上传大小限制常量
pub(crate) use imports::MAX_UPLOAD_BYTES;

// ============================================================
// router 函数
// ============================================================
/// 创建试卷管理模块的路由配置
///
/// # 返回
/// 配置好试卷相关路由的 Router
///
/// # 路由表
/// ```text
/// /papers
/// ├── /              GET (list) / POST (create)
/// ├── /:paper_id     GET (detail) / PATCH (update) / DELETE (delete)
/// └── /:paper_id/file  PUT (replace)
/// ```
pub(crate) fn router() -> Router<super::AppState> {
    // 创建新路由
    Router::new()
        // ====================================================
        // /papers 路由
        // ====================================================
        .route(
            "/papers",
            // GET /papers → 获取试卷列表（支持多种过滤）
            // POST /papers → 创建新试卷（上传 ZIP+ 元数据）
            get(handlers::list_papers)
                .post(handlers::create_paper),
        )
        // ====================================================
        // /papers/:paper_id 路由
        // ====================================================
        .route(
            "/papers/:paper_id",
            // GET → 获取试卷详情（含题目列表）
            get(handlers::get_paper_detail)
                // PATCH → 更新试卷元数据或题目顺序
                .patch(handlers::update_paper)
                // DELETE → 删除试卷
                .delete(handlers::delete_paper),
        )
        // ====================================================
        // /papers/:paper_id/file 路由
        // ====================================================
        .route(
            "/papers/:paper_id/file",
            // PUT → 替换试卷附加文件
            axum::routing::put(handlers::replace_paper_file),
        )
}

/*
 * ============================================================
 * 知识点：试卷 vs 题目 API 对比
 * ============================================================
 *
 * 相似点:
 * - 都支持 CRUD 操作
 * - 都支持文件上传和替换
 * - 都支持批量下载 (bundles)
 *
 * 不同点:
 * - 试卷创建需要关联已有题目
 * - 试卷有额外的验证规则（类别一致性）
 * - 试卷支持 LaTeX 渲染导出
 *
 * ============================================================
 * 试卷 API 端点一览
 * ============================================================
 *
 * GET /papers
 *   查询参数：question_id, category, tag, q (搜索), limit, offset
 *   返回：试卷摘要列表
 *
 * POST /papers
 *   Content-Type: multipart/form-data
 *   字段：file (ZIP), description, title, subtitle,
 *        authors (JSON 数组), reviewers (JSON 数组),
 *        question_ids (JSON 数组)
 *   返回：导入结果（paper_id, 状态）
 *
 * GET /papers/:id
 *   返回：试卷详情（含题目列表及顺序）
 *
 * PATCH /papers/:id
 *   Body: JSON (部分更新)
 *   可更新：description, title, subtitle, authors,
 *          reviewers, question_ids
 *
 * DELETE /papers/:id
 *   返回：删除结果
 *
 * PUT /papers/:id/file
 *   Content-Type: multipart/form-data
 *   字段：file (ZIP)
 *   返回：替换结果
 *
 * POST /papers/bundles
 *   Body: JSON { paper_ids: [...] }
 *   返回：ZIP 文件（含渲染后的 LaTeX 和资源）
 *
 */
