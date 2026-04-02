// ============================================================
// 文件：src/api/tests.rs
// 说明：API 集成测试
// ============================================================

//! API 查询参数和绑定数量的单元测试
//!
//! 验证题目和试卷查询的参数规范化、SQL 生成、绑定数量计算

#[cfg(test)]
mod tests {
    // 导入被测试的类型和函数
    use crate::api::{
        // 试卷模块：参数和绑定计数函数
        papers::{models::PapersParams, queries::count_paper_binds},
        // 题目模块：参数和绑定计数函数
        questions::{models::QuestionsParams, queries::count_question_binds},
    };

    // ============================================================
    // 题目查询测试
    // ============================================================
    #[test]
    fn question_query_normalizes_limit_offset_and_counts_binds() {
        // 构建测试参数
        let params = QuestionsParams {
            paper_id: Some("550e8400-e29b-41d4-a716-446655440000".into()),
            category: Some("none".into()),
            tag: Some("mechanics".into()),
            difficulty_tag: Some("human".into()),
            difficulty_min: Some(3),
            difficulty_max: Some(6),
            q: Some("pendulum".into()),
            limit: Some(999),     // 超过最大值 100
            offset: Some(-10),    // 负数
        };

        // 构建查询计划
        let query = params.build_query();

        // 验证 limit 被钳制到最大值 100
        assert_eq!(query.limit, 100);
        // 验证 offset 被钳制到最小值 0
        assert_eq!(query.offset, 0);
        // 验证绑定数量计算正确
        assert_eq!(query.bind_count, count_question_binds(&params));

        // 验证 SQL 包含必要的子句
        // 标签关联
        assert!(query.sql.contains("FROM question_tags qt"));
        // 难度关联
        assert!(query.sql.contains("FROM question_difficulties qd"));
        // 难度算法标签过滤
        assert!(query.sql.contains("qd.algorithm_tag = "));
        // 难度分数范围过滤
        assert!(query.sql.contains("qd.score >= "));
        assert!(query.sql.contains("qd.score <= "));
        // 试卷关联
        assert!(query.sql.contains("FROM paper_questions pq"));
        // 搜索使用 COALESCE 处理 NULL 描述
        assert!(query.sql.contains("COALESCE(q.description, '') ILIKE"));

        // 验证搜索不会匹配错误字段
        assert!(!query.sql.contains("q.question_id::text ILIKE"));
        assert!(!query.sql.contains("q.source_tex_path, '') ILIKE"));
    }

    // ============================================================
    // 试卷查询测试
    // ============================================================
    #[test]
    fn paper_query_normalizes_limit_offset_and_counts_binds() {
        // 构建测试参数
        let params = PapersParams {
            question_id: Some("550e8400-e29b-41d4-a716-446655440000".into()),
            category: Some("E".into()),
            tag: Some("optics".into()),
            q: Some("thermal".into()),
            limit: Some(999),     // 超过最大值 100
            offset: Some(-10),    // 负数
        };

        // 构建查询计划
        let query = params.build_query();

        // 验证 limit 被钳制到最大值 100
        assert_eq!(query.limit, 100);
        // 验证 offset 被钳制到最小值 0
        assert_eq!(query.offset, 0);
        // 验证绑定数量计算正确
        assert_eq!(query.bind_count, count_paper_binds(&params));

        // 验证 SQL 包含必要的子句
        // 题目关联
        assert!(query.sql.contains("FROM paper_questions pq"));
        // 标签关联
        assert!(query.sql.contains("JOIN question_tags qt"));
        // 搜索使用 CONCAT_WS 连接多个字段
        assert!(query.sql
            .contains("CONCAT_WS(' ', p.description, p.title, p.subtitle"));
    }
}

/*
 * ============================================================
 * 知识点讲解 (Rust 新手必读)
 * ============================================================
 *
 * 1. #[cfg(test)] 属性
 *    - 只在测试编译时包含代码
 *    - 生产编译时完全忽略
 *    - 测试代码不影响二进制大小
 *
 * 2. mod tests 约定
 *    - Rust 测试模块的标准命名
 *    - 通常放在文件末尾或被测试代码下方
 *    - 使用 super:: 访问父模块
 *
 * 3. 断言宏
 *    assert_eq!(a, b)     // a == b
 *    assert!(condition)   // condition 为真
 *    assert!(!condition)  // condition 为假
 *
 * 4. 测试函数命名
 *    - 使用下划线分隔的长名称
 *    - 描述测试内容
 *    - 例：question_query_normalizes_limit_offset_and_counts_binds
 *
 * 5. 测试运行
 *    cargo test           // 运行所有测试
 *    cargo test test_name // 运行指定测试
 *
 * ============================================================
 * 查询参数规范化规则
 * ============================================================
 *
 * limit:
 *   输入：任意 i64
 *   处理：.unwrap_or(20).clamp(1, 100)
 *   输出：1-100 之间，默认 20
 *
 * offset:
 *   输入：任意 i64
 *   处理：.unwrap_or(0).max(0)
 *   输出：>=0，默认 0
 *
 * ============================================================
 * SQL 生成验证要点
 * ============================================================
 *
 * 题目查询验证:
 * - 标签表关联 (question_tags)
 * - 难度表关联 (question_difficulties)
 * - 难度范围过滤 (score >= min, score <= max)
 * - 搜索使用 COALESCE 处理 NULL
 *
 * 试卷查询验证:
 * - 题目关联表 (paper_questions)
 * - 标签表关联 (question_tags)
 * - 搜索使用 CONCAT_WS 连接多字段
 *
 * ============================================================
 * 绑定数量验证的意义
 * ============================================================
 *
 * build_query() 生成 SQL 时使用 QueryBuilder.push_bind()
 * execute_query() 执行时使用 query.bind()
 *
 * 两者的绑定参数数量必须一致，否则：
 * - SQL 执行失败
 * - 参数错位导致错误数据
 *
 * 测试通过比较：
 * - query.bind_count (生成时计算)
 * - count_binds(&params) (独立函数重新计算)
 *
 * 确保两者逻辑同步更新。
 *
 */
