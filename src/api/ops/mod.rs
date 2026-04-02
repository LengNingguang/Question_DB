// ============================================================
// 文件：src/api/ops/mod.rs
// 说明：运维操作模块入口
// ============================================================

//! 运维操作 API 模块
//!
//! 提供批量打包、数据导出、质量检查等运维功能

// 声明子模块
pub(crate) mod bundles;      // 批量打包逻辑
pub(crate) mod exports;      // 数据导出 (CSV/JSONL)
pub(crate) mod handlers;     // HTTP 请求处理器
pub(crate) mod models;       // 请求/响应模型
pub(crate) mod paper_render; // LaTeX 渲染引擎
pub(crate) mod quality;      // 质量检查

// 导入 Axum 路由类型
use axum::{routing::post, Router};

// ============================================================
// router 函数
// ============================================================
/// 创建运维操作模块的路由配置
///
/// # 返回
/// 配置好运维相关路由的 Router
///
/// # 路由表
/// ```text
/// /questions/bundles    POST - 批量下载题目包
/// /papers/bundles       POST - 批量下载试卷包
/// /exports/run          POST - 导出数据
/// /quality-checks/run   POST - 运行质量检查
/// ```
pub(crate) fn router() -> Router<super::AppState> {
    // 创建新路由
    Router::new()
        // ====================================================
        // /questions/bundles 路由
        // ====================================================
        // POST /questions/bundles → 批量下载题目 ZIP 包
        // 请求体：{ question_ids: [...] }
        // 返回：ZIP 文件（含 manifest.json 和题目文件）
        .route(
            "/questions/bundles",
            post(handlers::download_questions_bundle),
        )
        // ====================================================
        // /papers/bundles 路由
        // ====================================================
        // POST /papers/bundles → 批量下载试卷 ZIP 包
        // 请求体：{ paper_ids: [...] }
        // 返回：ZIP 文件（含渲染后的 LaTeX、资源、附录）
        .route(
            "/papers/bundles",
            post(handlers::download_papers_bundle),
        )
        // ====================================================
        // /exports/run 路由
        // ====================================================
        // POST /exports/run → 导出题库数据
        // 请求体：{ format: "jsonl"|"csv", public: bool, output_path: string }
        // 返回：导出结果（文件路径、题目数量）
        .route("/exports/run", post(handlers::run_export))
        // ====================================================
        // /quality-checks/run 路由
        // ====================================================
        // POST /quality-checks/run → 运行数据质量检查
        // 请求体：{ output_path: string }
        // 返回：质量报告（缺失文件、空试卷等）
        .route("/quality-checks/run", post(handlers::run_quality_check))
}

/*
 * ============================================================
 * 知识点：运维操作的特点
 * ============================================================
 *
 * 1. 都是 POST 请求
 *    - 因为这些是"动作"而非资源操作
 *    - 遵循 RESTful 的 RPC 风格
 *
 * 2. 耗时长
 *    - 批量打包可能处理大量数据
 *    - 客户端需要设置较长的超时时间
 *
 * 3. 返回二进制或报告
 *    - bundles 返回 ZIP 文件
 *    - exports/quality 返回 JSON 报告
 *
 * 4. 依赖其他模块
 *    - bundles 依赖 questions 和 papers 的数据
 *    - paper_render 依赖 LaTeX 模板
 *
 * ============================================================
 * 运维 API 端点详解
 * ============================================================
 *
 * POST /questions/bundles
 * 用途：批量下载题目
 * 请求：{ question_ids: ["uuid1", "uuid2", ...] }
 * 响应：application/zip
 * 内容:
 *   - manifest.json (元数据)
 *   - {description}_{id}/
 *     ├── problem.tex
 *     └── assets/
 *
 * POST /papers/bundles
 * 用途：批量下载试卷
 * 请求：{ paper_ids: ["uuid1", "uuid2", ...] }
 * 响应：application/zip
 * 内容:
 *   - manifest.json (元数据)
 *   - {paperDesc}_{id}/
 *     ├── main.tex (渲染后的试卷)
 *     ├── append.zip (原始附录)
 *     └── assets/ (合并的资源)
 *
 * POST /exports/run
 * 用途：导出题库数据到文件
 * 请求：{ format: "jsonl", public: false, output_path: "/path" }
 * 响应：{ format, public, output_path, exported_questions }
 *
 * POST /quality-checks/run
 * 用途：检查数据完整性
 * 请求：{ output_path: "/path" }
 * 响应：{ output_path, report: { missing_tex_object, ... } }
 *
 */
