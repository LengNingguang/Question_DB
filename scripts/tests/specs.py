def build_problem_tex(title: str, prompt: str) -> str:
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


QUESTION_SPECS = [
    {
        "slug": "mechanics",
        "zip_name": "question_mechanics.zip",
        "tex_name": "mechanics.tex",
        "tex_body": build_problem_tex(
            "Mechanics calibration",
            "A cart slides on an incline and collides elastically with a block.",
        ),
        "create_description": "mechanics benchmark alpha",
        "create_difficulty": {
            "human": {
                "score": 2,
                "notes": "import baseline",
            }
        },
        "assets": {
            "assets/diagram.txt": "incline-figure",
            "assets/data.csv": "time,velocity\n0,0\n1,3\n",
        },
        "patch": {
            "category": "T",
            "description": "mechanics benchmark alpha",
            "tags": ["mechanics", "kinematics"],
            "status": "reviewed",
            "difficulty": {
                "human": {"score": 4, "notes": "warm-up"},
                "heuristic": {"score": 5, "notes": "fast estimate"},
                "ml": {"score": 3},
            },
        },
    },
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
            "category": "E",
            "description": "optics bundle beta",
            "tags": ["optics", "lenses"],
            "status": "used",
            "difficulty": {
                "human": {"score": 7, "notes": "competition-ready"},
                "heuristic": {"score": 6, "notes": "geometry-heavy"},
                "ml": {"score": 8, "notes": "vision model struggle"},
                "symbolic": {"score": 9},
            },
        },
    },
    {
        "slug": "thermal",
        "zip_name": "question_thermal.zip",
        "tex_name": "thermal.tex",
        "tex_body": build_problem_tex(
            "Thermal equilibration",
            "Two bodies exchange heat until they reach thermal equilibrium.",
        ),
        "create_description": "热学标定 gamma",
        "create_difficulty": {
            "human": {
                "score": 5,
            }
        },
        "assets": {
            "assets/table.txt": "material,c\nCu,385\nAl,900\n",
            "assets/reference.txt": "thermal-reference",
        },
        "patch": {
            "category": "none",
            "description": "热学标定 gamma",
            "tags": ["thermal", "calorimetry"],
            "status": "none",
            "difficulty": {
                "human": {"score": 5, "notes": ""},
                "heuristic": {"score": 4, "notes": "direct model"},
                "simulator": {"score": 6},
            },
        },
    },
]


PAPER_APPENDIX_SPECS = [
    {
        "slug": "mock-a",
        "zip_name": "paper_appendix_a.zip",
        "appendix_entries": {
            "meta/info.json": '{"version":1,"paper":"A"}',
            "drafts/notes.txt": "first draft appendix",
        },
    },
    {
        "slug": "mock-b",
        "zip_name": "paper_appendix_b.zip",
        "appendix_entries": {
            "review/summary.md": "# Thermal finals\n",
            "attachments/table.csv": "part,score\noptics,8\nthermal,10\n",
        },
    },
]

