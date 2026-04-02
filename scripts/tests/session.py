# ============================================================
# 文件：scripts/tests/session.py
# 说明：测试会话管理类
# ============================================================

"""
测试会话 (TestSession) 管理

封装 E2E 测试的完整生命周期：
- Docker 容器管理（PostgreSQL）
- API 服务启停
- HTTP 请求发送
- 日志记录和报告生成
"""

from __future__ import annotations

import io
import json
import shutil
import subprocess
import urllib.error
import urllib.request
import uuid
import zipfile
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from .config import (
    API_LOG_PATH,
    API_PORT,
    CONTAINER_NAME,
    DB_URL,
    DOWNLOADS_DIR,
    EXPORT_PATH,
    INVALID_PAPER_UPLOAD_PATH,
    ROOT_DIR,
    SAMPLES_DIR,
    TMP_DIR,
)


# ============================================================
# 工具函数：解析 JSON
# ============================================================
def parse_json(body: str) -> Any:
    """
    解析 JSON 响应体

    参数:
        body: 响应体字符串

    返回:
        解析后的 Python 对象（dict/list）或 None
    """
    return json.loads(body) if body else None


# ============================================================
# 工具函数：从响应体提取题目 ID
# ============================================================
def question_ids_from_body(body: str) -> list[str]:
    """
    从 JSON 响应体中提取题目 ID 列表

    参数:
        body: JSON 响应体

    返回:
        题目 ID 列表
    """
    return [item["question_id"] for item in parse_json(body)]


# ============================================================
# 工具函数：标准化响应头
# ============================================================
def normalize_headers(items) -> dict[str, str]:
    """
    将响应头转换为小写键的字典

    参数:
        items: 响应头元组列表 [(key, value), ...]

    返回:
        小写键的字典
    """
    return {key.lower(): value for key, value in items}


# ============================================================
# 工具函数：美化 JSON 输出
# ============================================================
def pretty_json(value: Any) -> str:
    """
    格式化 JSON 对象为美化字符串

    参数:
        value: Python 对象

    返回:
        缩进格式化的 JSON 字符串（已排序键）
    """
    return json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True)


# ============================================================
# 工具函数：格式化响应体
# ============================================================
def format_body(value: Any) -> tuple[str, str]:
    """
    格式化响应体为（类型，文本）元组

    参数:
        value: 响应体值

    返回:
        (类型，文本) 元组
        - 类型："json" 或 "text"
        - 文本：格式化后的字符串
    """
    if value is None:
        return "text", "(empty)"
    if isinstance(value, (dict, list)):
        return "json", pretty_json(value)
    return "text", str(value)


# ============================================================
# 工具函数：生成 Markdown 代码块
# ============================================================
def markdown_code_block(value: Any) -> str:
    """
    将值转换为 Markdown 代码块

    参数:
        value: 任意值

    返回:
        Markdown 代码块字符串
    """
    language, text = format_body(value)
    # JSON 使用 json 后缀，其他使用 text
    suffix = "json" if language == "json" else "text"
    return f"```{suffix}\n{text}\n```"


# ============================================================
# TestSession 数据类
# ============================================================
@dataclass
class TestSession:
    """
    测试会话类

    管理 E2E 测试的完整生命周期：
    - 工作区准备
    - Docker 容器管理
    - API 服务启停
    - HTTP 请求发送
    - 日志记录
    - 报告生成
    """
    # 请求日志列表：记录所有 HTTP 交换
    request_logs: list[dict[str, Any]] = field(default_factory=list)

    # 验证笔记列表：记录测试验证点
    validation_notes: list[str] = field(default_factory=list)

    # 样本输入列表：记录测试输入数据
    sample_inputs: list[dict[str, Any]] = field(default_factory=list)

    # API 进程对象
    api_process: subprocess.Popen | None = None

    # API 日志文件对象
    api_log_file: Any = None

    # ============================================================
    # 属性：目录路径
    # ============================================================
    @property
    def root_dir(self) -> Path:
        """项目根目录"""
        return ROOT_DIR

    @property
    def tmp_dir(self) -> Path:
        """临时目录（存放测试生成的所有文件）"""
        return TMP_DIR

    @property
    def samples_dir(self) -> Path:
        """样本目录（存放测试 ZIP 文件）"""
        return SAMPLES_DIR

    @property
    def downloads_dir(self) -> Path:
        """下载目录（存放 API 导出的 ZIP）"""
        return DOWNLOADS_DIR

    @property
    def api_log_path(self) -> Path:
        """API 日志文件路径"""
        return API_LOG_PATH

    @property
    def export_path(self) -> Path:
        """导出文件路径（JSONL）"""
        return EXPORT_PATH

    @property
    def quality_path(self) -> Path:
        """质量检查报告路径（JSON）"""
        return QUALITY_PATH

    @property
    def report_path(self) -> Path:
        """测试报告路径（Markdown）"""
        return REPORT_PATH

    @property
    def invalid_paper_upload_path(self) -> Path:
        """无效试卷上传路径（用于错误处理测试）"""
        return INVALID_PAPER_UPLOAD_PATH

    # ============================================================
    # 方法：打印步骤
    # ============================================================
    def print_step(self, label: str) -> None:
        """
        打印测试步骤标签

        参数:
            label: 步骤描述（如"[1/9] Build synthetic and real fixture zips"）
        """
        print(label, flush=True)

    # ============================================================
    # 方法：断言检查
    # ============================================================
    def ensure(self, condition: bool, message: str) -> None:
        """
        条件断言

        参数:
            condition: 条件表达式
            message: 失败时的错误消息

        异常:
            AssertionError: 当 condition 为 False 时抛出
        """
        if not condition:
            raise AssertionError(message)

    # ============================================================
    # 方法：注册测试输入
    # ============================================================
    def register_input(self, item: dict[str, Any]) -> None:
        """
        注册测试输入数据（用于报告生成）

        参数:
            item: 测试输入数据字典
        """
        self.sample_inputs.append(item)

    # ============================================================
    # 方法：运行命令
    # ============================================================
    def run_command(
        self,
        cmd: list[str],
        *,
        input_bytes: bytes | None = None,
        check: bool = True,
    ) -> subprocess.CompletedProcess:
        """
        运行 shell 命令

        参数:
            cmd: 命令和参数列表
            input_bytes: 标准输入（可选）
            check: 是否检查返回码（默认 True）

        返回:
            CompletedProcess 对象（包含 stdout、stderr、returncode）
        """
        return subprocess.run(
            cmd,
            cwd=self.root_dir,  # 工作目录
            input=input_bytes,  # 标准输入
            check=check,  # 失败时是否抛出异常
            stdout=subprocess.PIPE,  # 捕获标准输出
            stderr=subprocess.PIPE,  # 捕获标准错误
        )

    # ============================================================
    # 方法：准备工作区
    # ============================================================
    def prepare_workspace(self) -> None:
        """
        准备测试工作区

        操作:
        1. 删除旧的临时目录（如果存在）
        2. 创建样本目录
        3. 创建下载目录
        """
        # 递归删除临时目录（忽略不存在的错误）
        shutil.rmtree(self.tmp_dir, ignore_errors=True)

        # 创建样本目录（包括父目录）
        self.samples_dir.mkdir(parents=True, exist_ok=True)

        # 创建下载目录
        self.downloads_dir.mkdir(parents=True, exist_ok=True)

    # ============================================================
    # 方法：清理资源
    # ============================================================
    def cleanup(self) -> None:
        """
        清理测试资源

        操作:
        1. 终止 API 进程
        2. 关闭日志文件
        3. 删除 Docker 容器
        """
        # ========== 终止 API 进程 ==========
        if self.api_process is not None and self.api_process.poll() is None:
            # 优雅终止
            self.api_process.terminate()
            try:
                # 等待最多 5 秒
                self.api_process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                # 超时则强制杀死
                self.api_process.kill()
                self.api_process.wait(timeout=5)

        # ========== 关闭日志文件 ==========
        if self.api_log_file is not None and not self.api_log_file.closed:
            self.api_log_file.close()

        # ========== 删除 Docker 容器 ==========
        # 检查容器是否存在
        existing = self.run_command(
            ["docker", "ps", "-a", "--format", "{{.Names}}"],
            check=False,
        )

        # 如果存在则删除
        if CONTAINER_NAME in existing.stdout.decode().splitlines():
            self.run_command(["docker", "rm", "-f", CONTAINER_NAME], check=False)

    # ============================================================
    # 方法：记录请求日志
    # ============================================================
    def append_request_log(
        self,
        *,
        label: str,
        method: str,
        path: str,
        expected_status: int,
        actual_status: int,
        request_headers: dict[str, str],
        request_body: Any,
        response_headers: dict[str, str],
        response_body: Any,
    ) -> None:
        """
        添加 HTTP 请求日志

        参数:
            label: 请求标签（如"POST /questions (mechanics)"）
            method: HTTP 方法
            path: 请求路径
            expected_status: 期望的状态码
            actual_status: 实际的状态码
            request_headers: 请求头
            request_body: 请求体
            response_headers: 响应头
            response_body: 响应体
        """
        self.request_logs.append(
            {
                "label": label,
                "method": method,
                "path": path,
                "expected_status": expected_status,
                "actual_status": actual_status,
                "request_headers": request_headers,
                "request_body": request_body,
                "response_headers": response_headers,
                "response_body": response_body,
            }
        )

    # ============================================================
    # 方法：执行 HTTP 请求
    # ============================================================
    def perform_request(
        self,
        label: str,
        expected_status: int,
        *,
        method: str = "GET",
        path: str,
        headers: dict[str, str] | None = None,
        body: bytes | None = None,
        request_body: Any = None,
    ) -> tuple[int, str, dict[str, str]]:
        """
        执行 HTTP 请求并记录日志

        参数:
            label: 请求标签
            expected_status: 期望的状态码
            method: HTTP 方法（默认 GET）
            path: 请求路径
            headers: 请求头（可选）
            body: 请求体（字节）
            request_body: 请求体（Python 对象，用于日志）

        返回:
            (status, response_body, response_headers) 元组

        异常:
            RuntimeError: 当实际状态码与期望不符时
        """
        # 构建完整 URL
        url = f"http://127.0.0.1:{API_PORT}{path}"

        # 准备请求头
        request_headers = headers or {}

        # 创建请求对象
        request = urllib.request.Request(
            url, data=body, method=method, headers=request_headers
        )

        # ========== 发送请求 ==========
        try:
            # 正常响应
            with urllib.request.urlopen(request) as response:
                status = response.status
                response_headers = normalize_headers(response.headers.items())
                response_body = response.read().decode("utf-8")
        except urllib.error.HTTPError as err:
            # HTTP 错误（4xx/5xx）
            status = err.code
            response_headers = normalize_headers(err.headers.items())
            response_body = err.read().decode("utf-8", errors="replace")

        # ========== 记录日志 ==========
        self.append_request_log(
            label=label,
            method=method,
            path=path,
            expected_status=expected_status,
            actual_status=status,
            request_headers=request_headers,
            request_body=request_body,
            response_headers=response_headers,
            response_body=response_body,
        )

        # ========== 验证状态码 ==========
        if status != expected_status:
            raise RuntimeError(
                f"Unexpected status for {label}: expected {expected_status}, got {status}"
            )

        return status, response_body, response_headers

    # ============================================================
    # 方法：发送 JSON 请求
    # ============================================================
    def json_request(
        self,
        label: str,
        expected_status: int,
        *,
        method: str,
        path: str,
        payload: dict[str, Any],
    ) -> tuple[int, str, dict[str, str]]:
        """
        发送 JSON 请求

        参数:
            label: 请求标签
            expected_status: 期望的状态码
            method: HTTP 方法
            path: 请求路径
            payload: JSON 负载

        返回:
            (status, response_body, response_headers) 元组
        """
        # 序列化 JSON 并编码为字节
        body = json.dumps(payload, ensure_ascii=False).encode("utf-8")

        return self.perform_request(
            label,
            expected_status,
            method=method,
            path=path,
            headers={"content-type": "application/json"},  # 设置 Content-Type
            body=body,
            request_body=payload,  # 记录原始对象（用于日志）
        )

    # ============================================================
    # 方法：发送 multipart 请求
    # ============================================================
    def multipart_request(
        self,
        label: str,
        expected_status: int,
        *,
        method: str = "POST",
        path: str,
        text_fields: dict[str, str] | None,
        field_name: str | None = None,
        file_path: Path | None = None,
        content_type: str | None = None,
    ) -> tuple[int, str, dict[str, str]]:
        """
        发送 multipart/form-data 请求（文件上传）

        参数:
            label: 请求标签
            expected_status: 期望的状态码
            method: HTTP 方法（默认 POST）
            path: 请求路径
            text_fields: 文本字段（可选）
            field_name: 文件字段名（上传文件时需要）
            file_path: 文件路径（可选）
            content_type: 文件 MIME 类型（上传文件时需要）

        返回:
            (status, response_body, response_headers) 元组

        异常:
            ValueError: 当提供 file_path 但未提供 field_name 或 content_type 时
        """
        # 生成随机 boundary
        boundary = f"----QBApiBoundary{uuid.uuid4().hex}"

        # 构建请求体
        body = bytearray()

        # 添加文本字段
        for name, value in (text_fields or {}).items():
            body.extend(
                (
                    f"--{boundary}\r\n"
                    f'Content-Disposition: form-data; name="{name}"\r\n\r\n'
                    f"{value}\r\n"
                ).encode("utf-8")
            )

        # 添加文件字段
        if file_path is not None:
            # 验证必需参数
            if field_name is None or content_type is None:
                raise ValueError(
                    "field_name and content_type are required when file_path is provided"
                )

            # 读取文件内容
            file_bytes = file_path.read_bytes()

            # 添加文件头
            body.extend(
                (
                    f"--{boundary}\r\n"
                    f'Content-Disposition: form-data; name="{field_name}"; filename="{file_path.name}"\r\n'
                    f"Content-Type: {content_type}\r\n\r\n"
                ).encode("utf-8")
            )

            # 添加文件内容
            body.extend(file_bytes)
            body.extend(b"\r\n")

        # 添加结束标记
        body.extend(f"--{boundary}--\r\n".encode("utf-8"))

        # 发送请求
        return self.perform_request(
            label,
            expected_status,
            method=method,
            path=path,
            headers={"content-type": f"multipart/form-data; boundary={boundary}"},
            body=bytes(body),
            request_body={
                **({"file": str(file_path)} if file_path is not None else {}),
                **(text_fields or {}),
            },
        )

    # ============================================================
    # 方法：检查 ZIP 文件
    # ============================================================
    def inspect_zip_file(self, file_path: Path) -> dict[str, Any]:
        """
        检查 ZIP 文件内容

        参数:
            file_path: ZIP 文件路径

        返回:
            包含 entries 和 manifest 的字典
        """
        with zipfile.ZipFile(file_path, "r") as archive:
            # 获取所有条目名称
            names = archive.namelist()

            # 尝试读取 manifest.json
            manifest = None
            if "manifest.json" in names:
                manifest = json.loads(archive.read("manifest.json").decode("utf-8"))

        return {
            "entries": names,
            "manifest": manifest,
        }

    # ============================================================
    # 方法：检查 ZIP 字节
    # ============================================================
    def inspect_zip_bytes(self, data: bytes) -> list[str]:
        """
        从字节数据检查 ZIP 内容

        参数:
            data: ZIP 文件的字节数据

        返回:
            条目名称列表
        """
        with zipfile.ZipFile(io.BytesIO(data), "r") as archive:
            return archive.namelist()

    # ============================================================
    # 方法：二进制 JSON 请求
    # ============================================================
    def binary_json_request(
        self,
        label: str,
        expected_status: int,
        *,
        path: str,
        payload: dict[str, Any],
        output_path: Path,
    ) -> tuple[dict[str, Any], list[str]]:
        """
        发送 JSON 请求并将二进制响应保存为 ZIP 文件

        参数:
            label: 请求标签
            expected_status: 期望的状态码
            path: 请求路径
            payload: JSON 负载
            output_path: 输出 ZIP 文件路径

        返回:
            (manifest, entries) 元组
            - manifest: manifest.json 解析结果
            - entries: ZIP 条目列表

        异常:
            RuntimeError: 当状态码不匹配或缺少 manifest.json 时
        """
        # 序列化 JSON
        body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        request_headers = {"content-type": "application/json"}

        # 创建请求
        request = urllib.request.Request(
            f"http://127.0.0.1:{API_PORT}{path}",
            data=body,
            method="POST",
            headers=request_headers,
        )

        # ========== 发送请求 ==========
        try:
            with urllib.request.urlopen(request) as response:
                status = response.status
                response_headers = normalize_headers(response.headers.items())
                response_bytes = response.read()
        except urllib.error.HTTPError as err:
            # 处理错误响应
            status = err.code
            response_headers = normalize_headers(err.headers.items())
            response_body = err.read().decode("utf-8", errors="replace")

            # 记录日志
            self.append_request_log(
                label=label,
                method="POST",
                path=path,
                expected_status=expected_status,
                actual_status=status,
                request_headers=request_headers,
                request_body=payload,
                response_headers=response_headers,
                response_body=response_body,
            )

            # 抛出异常
            raise RuntimeError(
                f"Unexpected status for {label}: expected {expected_status}, got {status}"
            ) from err

        # ========== 保存响应 ==========
        # 创建父目录
        output_path.parent.mkdir(parents=True, exist_ok=True)

        # 写入文件
        output_path.write_bytes(response_bytes)

        # 检查 ZIP 内容
        zip_details = self.inspect_zip_file(output_path)

        # 验证 manifest.json 存在
        self.ensure(
            zip_details["manifest"] is not None,
            f"{label} should include manifest.json",
        )

        # ========== 记录日志 ==========
        self.append_request_log(
            label=label,
            method="POST",
            path=path,
            expected_status=expected_status,
            actual_status=status,
            request_headers=request_headers,
            request_body=payload,
            response_headers=response_headers,
            response_body={
                "saved_path": str(output_path),
                "entries": zip_details["entries"],
                "manifest": zip_details["manifest"],
            },
        )

        return zip_details["manifest"], zip_details["entries"]

    # ============================================================
    # 方法：等待 PostgreSQL
    # ============================================================
    def wait_for_postgres(self) -> None:
        """
        等待 PostgreSQL 容器就绪

        使用 pg_isready 命令轮询，最多等待 60 秒
        """
        for _ in range(60):
            # 执行 pg_isready 检查
            result = self.run_command(
                [
                    "docker",
                    "exec",
                    CONTAINER_NAME,
                    "pg_isready",
                    "-U",
                    "postgres",
                    "-d",
                    "qb",
                ],
                check=False,
            )

            # 成功则返回
            if result.returncode == 0:
                return

            # 等待 1 秒后重试
            import time
            time.sleep(1)

        # 60 次后仍未就绪，执行最后一次检查（会抛出异常）
        self.run_command(
            [
                "docker",
                "exec",
                CONTAINER_NAME,
                "pg_isready",
                "-U",
                "postgres",
                "-d",
                "qb",
            ]
        )

    # ============================================================
    # 方法：等待 API
    # ============================================================
    def wait_for_api(self) -> None:
        """
        等待 API 服务就绪

        通过轮询 /health 端点，最多等待 60 秒
        """
        for _ in range(60):
            try:
                # 尝试访问健康检查端点
                with urllib.request.urlopen(
                    f"http://127.0.0.1:{API_PORT}/health"
                ) as response:
                    # 状态码 200 表示就绪
                    if response.status == 200:
                        return
            except Exception:
                # 任何异常都等待 1 秒后重试
                import time
                time.sleep(1)

        # 60 次后仍未就绪，执行最后一次检查
        with urllib.request.urlopen(f"http://127.0.0.1:{API_PORT}/health") as response:
            self.ensure(response.status == 200, "health check should be 200")

    # ============================================================
    # 方法：启动 PostgreSQL 容器
    # ============================================================
    def start_postgres_container(self) -> None:
        """
        启动 PostgreSQL Docker 容器

        操作:
        1. 检查并删除已存在的同名容器
        2. 创建新容器
        3. 等待容器就绪
        """
        # 检查是否已有同名容器
        existing = self.run_command(
            ["docker", "ps", "-a", "--format", "{{.Names}}"],
            check=False,
        )

        # 删除已存在的容器
        if CONTAINER_NAME in existing.stdout.decode().splitlines():
            self.run_command(["docker", "rm", "-f", CONTAINER_NAME], check=False)

        # 启动新容器
        self.run_command(
            [
                "docker",
                "run",
                "-d",  # 后台运行
                "--name",
                CONTAINER_NAME,
                "-e",
                "POSTGRES_USER=postgres",  # 用户名
                "-e",
                "POSTGRES_PASSWORD=postgres",  # 密码
                "-e",
                "POSTGRES_DB=qb",  # 数据库名
                "-p",
                f"{POSTGRES_PORT}:5432",  # 端口映射
                POSTGRES_IMAGE,  # 镜像名
            ]
        )

        # 等待 PostgreSQL 就绪
        self.wait_for_postgres()

    # ============================================================
    # 方法：应用数据库迁移
    # ============================================================
    def apply_migration(self) -> None:
        """
        应用数据库迁移脚本

        读取 migrations/0001_init_pg.sql 并通过 psql 执行
        """
        # 读取迁移脚本
        migration_bytes = (
            self.root_dir / "migrations" / "0001_init_pg.sql"
        ).read_bytes()

        # 通过 docker exec 执行 psql
        self.run_command(
            [
                "docker",
                "exec",
                "-i",  # 交互模式（从 stdin 读取）
                CONTAINER_NAME,
                "psql",
                "-U",
                "postgres",
                "-d",
                "qb",
            ],
            input_bytes=migration_bytes,  # 通过 stdin 传递 SQL
        )

    # ============================================================
    # 方法：启动 API 服务
    # ============================================================
    def start_api(self) -> None:
        """
        启动 QB API 服务

        操作:
        1. 打开日志文件
        2. 设置环境变量
        3. 启动 cargo run 进程
        4. 等待 API 就绪
        """
        # 打开日志文件（二进制写入）
        self.api_log_file = self.api_log_path.open("wb")

        import os

        # 复制当前环境变量
        env = dict(**os.environ)

        # 设置数据库 URL
        env["QB_DATABASE_URL"] = DB_URL

        # 设置监听地址
        env["QB_BIND_ADDR"] = f"127.0.0.1:{API_PORT}"

        # 启动 API 进程
        self.api_process = subprocess.Popen(
            ["cargo", "run"],  # Rust 项目启动命令
            cwd=self.root_dir,
            env=env,
            stdout=self.api_log_file,  # 标准输出重定向到日志
            stderr=subprocess.STDOUT,  # 标准错误合并到标准输出
        )

        # 等待 API 就绪
        self.wait_for_api()

    # ============================================================
    # 方法：编写测试报告
    # ============================================================
    def write_report(self, status: str, error_text: str | None) -> None:
        """
        生成 Markdown 格式的测试报告

        参数:
            status: 测试状态（"passed" 或 "failed"）
            error_text: 错误堆栈文本（失败时）
        """
        # 生成时间戳（UTC）
        generated_at = datetime.now(timezone.utc).isoformat()

        # 构建报告行列表
        lines = [
            "# QB E2E Report",
            "",
            f"- Generated at: `{generated_at}`",
            f"- Status: `{status}`",
            f"- Report path: `{self.report_path}`",
            f"- Artifacts directory: `{self.tmp_dir}`",
            f"- API log: `{self.api_log_path}`",
            f"- Export output: `{self.export_path}`",
            f"- Quality output: `{self.quality_path}`",
            f"- Downloaded zips directory: `{self.downloads_dir}`",
            "",
            "## Sample Inputs",
            "",
            # 样本输入（JSON 格式）
            markdown_code_block(self.sample_inputs),
            "",
            "## Validation Notes",
            "",
        ]

        # 添加验证笔记
        if self.validation_notes:
            lines.extend([f"- {note}" for note in self.validation_notes])
        else:
            lines.append("- No validation notes recorded.")

        # 添加错误信息（如果失败）
        if error_text:
            lines.extend(
                [
                    "",
                    "## Failure",
                    "",
                    "```text",
                    error_text.rstrip(),
                    "```",
                ]
            )

        # 添加 HTTP 交换日志
        lines.extend(["", "## HTTP Exchanges", ""])

        # 遍历每个请求日志
        for index, entry in enumerate(self.request_logs, start=1):
            lines.extend(
                [
                    f"### {index}. {entry['label']}",
                    "",
                    f"- Request: `{entry['method']} {entry['path']}`",
                    f"- Expected status: `{entry['expected_status']}`",
                    f"- Actual status: `{entry['actual_status']}`",
                    "",
                    "#### Request Headers",
                    "",
                    markdown_code_block(entry["request_headers"] or {}),
                    "",
                    "#### Request Body",
                    "",
                    markdown_code_block(entry["request_body"]),
                    "",
                    "#### Response Headers",
                    "",
                    markdown_code_block(entry["response_headers"] or {}),
                    "",
                    "#### Response Body",
                    "",
                    markdown_code_block(entry["response_body"]),
                    "",
                ]
            )

        # 写入文件
        self.report_path.write_text("\n".join(lines), encoding="utf-8")


"""
============================================================
知识点讲解 (Python 测试会话管理)
============================================================

1. @dataclass 装饰器
   @dataclass
   class TestSession:
       request_logs: list = field(default_factory=list)

   - 自动生成__init__方法
   - field(default_factory=list) 为每个实例创建独立列表
   - 避免可变默认参数的陷阱

2. @property 装饰器
   @property
   def root_dir(self) -> Path:
       return ROOT_DIR

   - 将方法转换为属性访问
   - 调用时使用 session.root_dir 而非 session.root_dir()

3. urllib.request 模块
   urllib.request.Request(url, data, method, headers)
   urllib.request.urlopen(request)

   - Python 内置的 HTTP 客户端
   - 无需额外安装依赖
   - 支持 GET/POST/PUT/DELETE 等方法

4. subprocess 模块
   subprocess.run(cmd, input, check, stdout, stderr)
   subprocess.Popen(cmd, env, stdout, stderr)

   - run(): 运行命令并等待完成
   - Popen(): 启动进程（不阻塞）

5. zipfile 模块
   ZipFile(path, "r") 读取
   ZipFile(path, "w") 写入
   archive.namelist() 获取条目列表
   archive.read(path) 读取文件内容
   archive.writestr(name, data) 写入字符串

============================================================
TestSession 生命周期
============================================================

┌─────────────────────┐
│ prepare_workspace() │ 清理并创建目录
└──────────┬──────────┘
           │
┌──────────▼──────────┐
│   start_postgres()  │ 启动 Docker 容器
└──────────┬──────────┘
           │
┌──────────▼──────────┐
│   apply_migration() │ 执行 SQL 迁移
└──────────┬──────────┘
           │
┌──────────▼──────────┐
│     start_api()     │ 启动 Rust API
└──────────┬──────────┘
           │
┌──────────▼──────────┐
│   执行测试请求       │ HTTP 请求/验证
└──────────┬──────────┘
           │
┌──────────▼──────────┐
│     cleanup()       │ 清理资源
└──────────┬──────────┘
           │
┌──────────▼──────────┐
│   write_report()    │ 生成 Markdown 报告
└─────────────────────┘

============================================================
HTTP 请求方法对比
============================================================

| 方法               | 用途                    |
|--------------------|-------------------------|
| perform_request()  | 通用请求（底层）        |
| json_request()     | JSON 请求（自动序列化） |
| multipart_request()| 文件上传（multipart）   |
| binary_json_request()| JSON 请求 + 二进制响应 |

============================================================
错误处理策略
============================================================

1. HTTP 错误（4xx/5xx）
   - 捕获 urllib.error.HTTPError
   - 读取错误响应体
   - 记录日志
   - 状态码不匹配时抛出 RuntimeError

2. Docker 命令失败
   - check=False 允许失败
   - 通过 returncode 判断
   - 超时处理（wait(timeout=5)）

3. 断言失败
   - ensure(condition, message)
   - 抛出 AssertionError
   - 包含清晰的错误消息

============================================================
报告生成内容
============================================================

Markdown 报告包含：

1. 头部信息
   - 生成时间
   - 测试状态
   - 文件路径

2. 样本输入
   - 所有测试输入数据（JSON 格式）

3. 验证笔记
   - 测试过程中的关键验证点

4. 失败信息（如果有）
   - 完整的错误堆栈

5. HTTP 交换日志
   - 每个请求的详细信息
   - 请求头/请求体
   - 响应头/响应体
"""
