# ============================================================
# 文件：scripts/tests/specs.py
# 说明：测试数据规格定义
# ============================================================

"""
测试数据规格

定义合成题目和试卷附录的数据规格，用于生成测试用的 ZIP 文件
"""

#!/usr/bin/env python3

import sys
from pathlib import Path


# ============================================================
# 路径配置
# ============================================================
# 项目根目录：当前文件的上上级目录
ROOT_DIR = Path(__file__).resolve().parent.parent

# 将根目录添加到 Python 路径（支持从 scripts 目录导入）
if str(ROOT_DIR) not in sys.path:
    sys.path.insert(0, str(ROOT_DIR))

# 导入主测试流程函数
from scripts.tests.full_flow import main


if __name__ == "__main__":
    main()


# ============================================================
# LaTeX 模板生成函数
# ============================================================
def build_problem_tex(title: str, prompt: str) -> str:
    """
    构建简单的 LaTeX 题目模板

    参数:
        title: 题目标题
        prompt: 题目描述

    返回:
        完整的 LaTeX 文档字符串，包含 problem 环境
    """
    # 使用 raw string (r"") 避免转义反斜杠
    # rf"" 支持 f-string 插值和 raw string
    return rf"""\documentclass[answer]{{cphos}}
\cphostitle{{QB API E2E}}
\cphossubtitle{{Synthetic Question}}
\setscorecheck{{true}}
\begin{{document}}
\begin{{problem}}[20]{{{title}}}
\begin{{problemstatement}}
{prompt}

\subq{{1}} 请使用式\ref{{eq:main}}完成简单计算。
\end{{problemstatement}}
\begin{{solution}}
\solsubq{{1}}{{20}}
\begin{{equation}}
1 + 1 = 2 \label{{eq:main}}
\end{{equation}}
\addtext{{说明}}{{18}}
\end{{solution}}
\end{{problem}}
\end{{document}}
"""


# ============================================================
# 题目规格定义
# ============================================================
QUESTION_SPECS = [
    # --------------------------------------------------------
    # 题目 1: 力学题 (mechanics)
    # --------------------------------------------------------
    {
        #  slug: 用于标识题目的简短名称
        "slug": "mechanics",

        # ZIP 文件名：上传到 API 的文件名
        "zip_name": "question_mechanics.zip",

        # TeX 文件名：ZIP 内部的主 TeX 文件
        "tex_name": "mechanics.tex",

        # TeX 内容：使用模板生成，包含 problem 环境
        "tex_body": build_problem_tex(
            "Mechanics calibration",  # 标题
            "A cart slides on an incline and collides elastically with a block.",  # 描述
        ),

        # 创建时的描述（用于搜索测试）
        "create_description": "mechanics benchmark alpha",

        # 创建时的难度定义（必须包含 human 评估）
        "create_difficulty": {
            "human": {
                "score": 2,  # 难度分数 1-10
                "notes": "import baseline",  # 备注
            }
        },

        # 资源文件：ZIP 内部的 assets 目录
        "assets": {
            "assets/diagram.txt": "incline-figure",  # 示意图
            "assets/data.csv": "time,velocity\n0,0\n1,3\n",  # 数据文件
        },

        # PATCH 请求的数据：更新题目元数据
        "patch": {
            "category": "T",  # 理论题
            "description": "mechanics benchmark alpha",
            "tags": ["mechanics", "kinematics"],  # 标签
            "status": "reviewed",  # 已通过审核
            "difficulty": {
                "human": {"score": 4, "notes": "warm-up"},
                "heuristic": {"score": 5, "notes": "fast estimate"},
                "ml": {"score": 3},  # 机器学习评估
            },
        },
    },

    # --------------------------------------------------------
    # 题目 2: 光学题 (optics)
    # --------------------------------------------------------
    {
        "slug": "optics",
        "zip_name": "question_optics.zip",
        "tex_name": "optics.tex",
        "tex_body": build_problem_tex(
            "Optics setup",
            "A lens forms an image on a screen and the magnification is to be derived.",
        ),
        "create_description": "optics bundle beta",
        "create_difficulty": {
            "human": {
                "score": 6,
                "notes": "import triage",
            }
        },
        "assets": {
            "assets/lens.txt": "thin-lens",
            "assets/ray-path.txt": "ray-diagram",
        },
        "patch": {
            "category": "E",  # 实验题
            "description": "optics bundle beta",
            "tags": ["optics", "lenses"],
            "status": "used",  # 已使用
            "difficulty": {
                "human": {"score": 7, "notes": "competition-ready"},
                "heuristic": {"score": 6, "notes": "geometry-heavy"},
                "ml": {"score": 8, "notes": "vision model struggle"},
                "symbolic": {"score": 9},  # 符号计算评估
            },
        },
    },

    # --------------------------------------------------------
    # 题目 3: 热学题 (thermal) - 包含中文字符
    # --------------------------------------------------------
    {
        "slug": "thermal",
        "zip_name": "question_thermal.zip",
        "tex_name": "thermal.tex",
        "tex_body": build_problem_tex(
            "Thermal equilibration",
            "Two bodies exchange heat until they reach thermal equilibrium.",
        ),
        # 中文描述：测试 UTF-8 编码
        "create_description": "热学标定 gamma",
        "create_difficulty": {
            "human": {
                "score": 5,
                # 空备注测试
            }
        },
        "assets": {
            "assets/table.txt": "material,c\nCu,385\nAl,900\n",  # 比热容表
            "assets/reference.txt": "thermal-reference",
        },
        "patch": {
            "category": "none",  # 未分类
            "description": "热学标定 gamma",
            "tags": ["thermal", "calorimetry"],
            "status": "none",  # 未审核
            "difficulty": {
                "human": {"score": 5, "notes": ""},
                "heuristic": {"score": 4, "notes": "direct model"},
                "simulator": {"score": 6},  # 模拟器评估
            },
        },
    },
]


# ============================================================
# 试卷附录规格定义
# ============================================================
PAPER_APPENDIX_SPECS = [
    # --------------------------------------------------------
    # 试卷附录 A: mock-a
    # --------------------------------------------------------
    {
        "slug": "mock-a",
        "zip_name": "paper_appendix_a.zip",
        # ZIP 内部的条目
        "appendix_entries": {
            "meta/info.json": '{"version":1,"paper":"A"}',  # 元信息
            "drafts/notes.txt": "first draft appendix",  # 草稿笔记
        },
    },

    # --------------------------------------------------------
    # 试卷附录 B: mock-b
    # --------------------------------------------------------
    {
        "slug": "mock-b",
        "zip_name": "paper_appendix_b.zip",
        "appendix_entries": {
            "review/summary.md": "# Thermal finals\n",  # Markdown 总结
            "attachments/table.csv": "part,score\noptics,8\nthermal,10\n",  # 成绩表
        },
    },
]


"""
============================================================
知识点讲解 (Python 测试规格)
============================================================

1. f-string 与 raw string 结合
   rf"""...{variable}..."""
   - r 前缀：反斜杠不转义（LaTeX 需要）
   - f 前缀：支持{variable}插值
   - 双花括号{{ }} 转义为单个{}

2. 列表包含字典的数据结构
   QUESTION_SPECS = [{...}, {...}, {...}]
   - 每个字典定义一道题目的完整规格
   - 通过遍历列表生成多个测试文件

3. 嵌套字典访问
   spec["patch"]["difficulty"]["human"]["score"]
   - 多层嵌套组织数据
   - 测试时用于构建 API 请求

============================================================
题目规格设计思路
============================================================

每道题目包含以下维度：

1. 基础信息:
   - slug: 人类可读的标识符
   - zip_name: 上传文件名
   - tex_name: TeX 源文件名

2. 内容生成:
   - tex_body: LaTeX 文档内容
   - assets: 资源文件路径→内容映射

3. 创建参数:
   - create_description: 初始描述
   - create_difficulty: 初始难度（必须有 human）

4. 更新参数 (PATCH):
   - category: T(理论) / E(实验) / none
   - status: reviewed(已审核) / used(已使用) / none
   - tags: 标签数组
   - difficulty: 多来源难度评估

============================================================
难度评估来源说明
============================================================

| 来源       | 说明                    |
|------------|-------------------------|
| human      | 人工评估（必须）        |
| heuristic  | 启发式算法评估          |
| ml         | 机器学习模型评估        |
| symbolic   | 符号计算引擎评估        |
| simulator  | 模拟器测试评估          |

分数范围：1-10（10 为最难）

============================================================
测试覆盖场景
============================================================

mechanics (力学):
  - 分类：T (理论题)
  - 状态：reviewed (已审核)
  - 测试：基础 CRUD、标签查询、难度查询

optics (光学):
  - 分类：E (实验题)
  - 状态：used (已使用)
  - 测试：多难度来源、分类过滤

thermal (热学):
  - 分类：none (未分类)
  - 状态：none (未审核)
  - 测试：中文字符、空备注、范围查询
"""
