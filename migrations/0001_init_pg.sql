-- ============================================================
-- 文件：migrations/0001_init_pg.sql
-- 说明：PostgreSQL 数据库初始架构定义
-- ============================================================

-- PostgreSQL 题库系统初始架构
-- 这个迁移文件创建了所有必需的表结构

-- ============================================================
-- objects 表：文件存储表
-- ============================================================
-- 存储所有上传文件的二进制内容和元数据
-- 包括题目 TeX 源文件、图片资源、试卷附加文件等
CREATE TABLE IF NOT EXISTS objects (
    -- object_id: 文件唯一标识符，使用 UUID 类型
    -- UUID 是通用唯一标识符，格式如：550e8400-e29b-41d4-a716-446655440000
    object_id UUID PRIMARY KEY,

    -- file_name: 原始文件名
    -- TEXT 类型可以存储任意长度的字符串
    file_name TEXT NOT NULL,

    -- mime_type: 文件 MIME 类型 (可选)
    -- 如 "text/x-tex", "image/png", "application/zip"
    mime_type TEXT,

    -- size_bytes: 文件大小 (字节)
    -- BIGINT 可以存储大文件 (最大 9EB)
    -- CHECK 约束确保值非负
    size_bytes BIGINT NOT NULL CHECK (size_bytes >= 0),

    -- content: 文件二进制内容
    -- BYTEA 是 PostgreSQL 的二进制数据类型
    content BYTEA NOT NULL,

    -- created_at: 创建时间
    -- TIMESTAMPTZ 是带时区的时间戳
    -- DEFAULT NOW() 自动设置为当前时间
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ============================================================
-- questions 表：题目元数据表
-- ============================================================
-- 存储每道题目的基本信息
CREATE TABLE IF NOT EXISTS questions (
    -- question_id: 题目唯一标识符
    question_id UUID PRIMARY KEY,

    -- source_tex_path: 源 TeX 文件路径
    -- 记录原始上传的 TeX 文件名
    source_tex_path TEXT NOT NULL,

    -- category: 题目分类
    -- 'none': 未分类
    -- 'T': Theory (理论题)
    -- 'E': Experiment (实验题)
    -- CHECK 约束确保只能是这三个值之一
    category TEXT NOT NULL DEFAULT 'none'
        CHECK (category IN ('none', 'T', 'E')),

    -- status: 题目状态
    -- 'none': 新上传，未审核
    -- 'reviewed': 已审核，可加入试卷
    -- 'used': 已使用过
    status TEXT NOT NULL DEFAULT 'none'
        CHECK (status IN ('none', 'reviewed', 'used')),

    -- description: 题目描述
    -- 用于搜索和标识
    -- CHECK 约束确保不能为空或仅空白
    description TEXT NOT NULL CHECK (btrim(description) <> ''),

    -- created_at: 创建时间
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- updated_at: 最后更新时间
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ============================================================
-- question_files 表：题目文件关联表
-- ============================================================
-- 关联题目和文件 (TeX 源文件、资源文件)
CREATE TABLE IF NOT EXISTS question_files (
    -- question_file_id: 记录唯一标识
    question_file_id UUID PRIMARY KEY,

    -- question_id: 关联的题目
    -- REFERENCES 定义外键约束
    -- ON DELETE CASCADE: 题目删除时，文件记录也删除
    question_id UUID NOT NULL
        REFERENCES questions(question_id) ON DELETE CASCADE,

    -- object_id: 关联的文件对象
    -- REFERENCES 定义外键约束
    -- ON DELETE CASCADE: 文件删除时，记录也删除
    object_id UUID NOT NULL
        REFERENCES objects(object_id) ON DELETE CASCADE,

    -- file_kind: 文件类型
    -- 'tex': TeX 源文件
    -- 'asset': 资源文件 (图片等)
    file_kind TEXT NOT NULL CHECK (file_kind IN ('tex', 'asset')),

    -- file_path: 文件在 ZIP 中的原始路径
    file_path TEXT NOT NULL,

    -- mime_type: 文件 MIME 类型
    mime_type TEXT,

    -- created_at: 创建时间
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- 唯一约束：同一题目的同类型同路径文件只能有一个
    UNIQUE (question_id, file_kind, file_path)
);

-- ============================================================
-- question_tags 表：题目标签表
-- ============================================================
-- 存储题目的标签列表 (如 "力学", "光学", "2024 真题")
CREATE TABLE IF NOT EXISTS question_tags (
    -- question_id: 关联的题目
    question_id UUID NOT NULL
        REFERENCES questions(question_id) ON DELETE CASCADE,

    -- tag: 标签文本
    tag TEXT NOT NULL,

    -- sort_order: 排序顺序
    -- 用于控制标签显示顺序
    sort_order INT NOT NULL,

    -- 复合主键：(question_id, tag) 唯一
    PRIMARY KEY (question_id, tag),

    -- 唯一约束：同一题目的排序位置唯一
    UNIQUE (question_id, sort_order)
);

-- ============================================================
-- question_difficulties 表：题目难度表
-- ============================================================
-- 存储不同评估方法对题目的难度评分
CREATE TABLE IF NOT EXISTS question_difficulties (
    -- question_id: 关联的题目
    question_id UUID NOT NULL
        REFERENCES questions(question_id) ON DELETE CASCADE,

    -- algorithm_tag: 评估方法标签
    -- 'human': 人工评估
    -- 'ml': 机器学习模型评估
    -- 'heuristic': 启发式评估
    -- 可以有任意多个不同的评估方法
    algorithm_tag TEXT NOT NULL,

    -- score: 难度分数
    -- 范围 1-10，10 为最难
    score INT NOT NULL CHECK (score BETWEEN 1 AND 10),

    -- notes: 评估备注 (可选)
    -- 记录评估的详细说明
    notes TEXT,

    -- 复合主键：(question_id, algorithm_tag) 唯一
    -- 同一评估方法对同一题目只能有一个评分
    PRIMARY KEY (question_id, algorithm_tag)
);

-- ============================================================
-- papers 表：试卷表
-- ============================================================
-- 存储试卷的基本信息
CREATE TABLE IF NOT EXISTS papers (
    -- paper_id: 试卷唯一标识符
    paper_id UUID PRIMARY KEY,

    -- description: 试卷描述
    -- 用于 bundle 命名和搜索
    description TEXT NOT NULL CHECK (btrim(description) <> ''),

    -- title: 试卷标题
    title TEXT NOT NULL CHECK (btrim(title) <> ''),

    -- subtitle: 试卷副标题
    subtitle TEXT NOT NULL CHECK (btrim(subtitle) <> ''),

    -- authors: 命题人数组
    -- TEXT[] 是 PostgreSQL 的数组类型
    -- DEFAULT '{}' 默认为空数组
    authors TEXT[] NOT NULL DEFAULT '{}',

    -- reviewers: 审题人数组
    reviewers TEXT[] NOT NULL DEFAULT '{}',

    -- append_object_id: 附加文件 ID
    -- 存储创建试卷时上传的 ZIP 文件
    -- NOT NULL 确保每份试卷都有附加文件
    append_object_id UUID NOT NULL
        REFERENCES objects(object_id),

    -- created_at: 创建时间
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- updated_at: 最后更新时间
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ============================================================
-- paper_questions 表：试卷题目关联表
-- ============================================================
-- 存储试卷和题目的有序关联
CREATE TABLE IF NOT EXISTS paper_questions (
    -- paper_id: 关联的试卷
    paper_id UUID NOT NULL
        REFERENCES papers(paper_id) ON DELETE CASCADE,

    -- question_id: 关联的题目
    question_id UUID NOT NULL
        REFERENCES questions(question_id) ON DELETE CASCADE,

    -- sort_order: 题目在试卷中的顺序
    -- 从 1 开始递增
    sort_order INT NOT NULL,

    -- created_at: 创建时间
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- 复合主键：(paper_id, question_id) 唯一
    -- 同一题目在同一试卷中只能出现一次
    PRIMARY KEY (paper_id, question_id),

    -- 唯一约束：同一试卷的排序位置唯一
    UNIQUE (paper_id, sort_order)
);

-- ============================================================
-- 索引创建
-- ============================================================
-- 索引加速查询，但会增加写入开销

-- questions 表的 status 列索引
-- 用于快速查询某状态的题目 (如查询所有 reviewed 题目)
CREATE INDEX IF NOT EXISTS idx_questions_status ON questions(status);

-- question_files 表的 question_id 索引
-- 用于快速查询某题目的所有文件
CREATE INDEX IF NOT EXISTS idx_question_files_question_id
    ON question_files(question_id);

-- question_tags 表的 question_id 索引
-- 用于快速查询某题目的所有标签
CREATE INDEX IF NOT EXISTS idx_question_tags_question_id
    ON question_tags(question_id);

-- question_difficulties 表的 question_id 索引
-- 用于快速查询某题目的所有难度评分
CREATE INDEX IF NOT EXISTS idx_question_difficulties_question_id
    ON question_difficulties(question_id);

-- question_difficulties 表的 (algorithm_tag, score) 索引
-- 用于按难度标签和分数范围查询 (如查询 human 评估 5-8 分的题目)
CREATE INDEX IF NOT EXISTS idx_question_difficulties_algorithm_tag_score
    ON question_difficulties(algorithm_tag, score);

-- paper_questions 表的 paper_id 索引
-- 用于快速查询某试卷的所有题目
CREATE INDEX IF NOT EXISTS idx_paper_questions_paper_id
    ON paper_questions(paper_id);

-- paper_questions 表的 question_id 索引
-- 用于快速查询包含某题目的所有试卷
CREATE INDEX IF NOT EXISTS idx_paper_questions_question_id
    ON paper_questions(question_id);

/*
 * ============================================================
 * 数据库设计要点
 * ============================================================
 *
 * 1. UUID 主键
 *    - 全局唯一，不暴露数据量
 *    - 适合分布式系统
 *    - 比自增 ID 更安全
 *
 * 2. 外键约束
 *    - 保证数据一致性
 *    - ON DELETE CASCADE 自动清理关联数据
 *    - 防止孤儿记录
 *
 * 3. CHECK 约束
 *    - 在数据库层面验证数据
 *    - 防止非法值写入
 *    - 减少应用层验证负担
 *
 * 4. 索引策略
 *    - 为常用查询列创建索引
 *    - 复合索引优化特定查询
 *    - 平衡读写性能
 *
 * 5. 时间戳
 *    - TIMESTAMPTZ 存储 UTC 时间
 *    - 自动处理时区转换
 *    - 便于跨时区协作
 *
 * ============================================================
 * 表关系图
 * ============================================================
 *
 * objects (文件存储)
 *     ↑
 *     | (objects 被引用)
 *     |
 * questions ←→ question_files (题目文件关联)
 *     ↑              ↑
 *     |              |
 *     |         question_tags (标签)
 *     |         question_difficulties (难度)
 *     |
 * papers ←→ paper_questions (试卷题目关联)
 *     ↑
 *     |
 * append_object_id → objects
 *
 */
