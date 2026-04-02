// ============================================================
// 文件：src/config.rs
// 说明：从环境变量加载应用配置
// ============================================================

//! 运行时配置模块，从环境变量加载配置
//!
//! 使用环境变量而非配置文件的原因：
//! - 便于容器化部署 (Docker, Kubernetes)
//! - 支持 12-Factor App 原则
//! - 不同环境使用不同配置而无需修改代码

// 导入标准库的环境变量模块
use std::{env, net::SocketAddr};

// 导入 anyhow 错误处理库
// anyhow 提供通用的错误类型，简化错误处理
use anyhow::{Context, Result};

// ============================================================
// AppConfig 结构体
// ============================================================
// 定义应用配置的数据结构
// #[derive(...)] 是派生宏，自动生成常用 trait 的实现
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    /// PostgreSQL 数据库连接 URL
    /// 格式：postgres://用户名：密码@主机：端口/数据库名
    pub database_url: String,

    /// HTTP 服务器监听地址
    /// SocketAddr 包含 IP 地址和端口号
    pub bind_addr: SocketAddr,
}

// ============================================================
// AppConfig 的实现块
// ============================================================
impl AppConfig {
    /// 从环境变量加载配置
    ///
    /// # 返回
    /// - Ok(AppConfig): 配置加载成功
    /// - Err(anyhow::Error): 配置加载失败
    ///
    /// # 环境变量
    /// - QB_DATABASE_URL: 必须设置，数据库连接字符串
    /// - QB_BIND_ADDR: 可选，默认 "127.0.0.1:8080"
    pub fn from_env() -> Result<Self> {
        // ====================================================
        // 读取数据库连接 URL
        // ====================================================
        // env::var() 读取环境变量
        // 返回 Result<String, VarError>
        // .context() 在错误时添加额外描述信息
        let database_url =
            env::var("QB_DATABASE_URL")
                .context("QB_DATABASE_URL is required for Rust API")?;

        // ====================================================
        // 读取服务器绑定地址
        // ====================================================
        // 使用 env::var() 读取，如果不存在则使用默认值
        // .unwrap_or_else() 在环境变量不存在时提供默认值
        let bind_addr = env::var("QB_BIND_ADDR")
            // 默认绑定到本地 8080 端口
            .unwrap_or_else(|_| "127.0.0.1:8080".to_string())
            // 将字符串解析为 SocketAddr 类型
            // .parse() 需要类型实现 FromStr trait
            .parse()
            // 如果解析失败，返回带上下文的错误
            .context("QB_BIND_ADDR must be a valid socket address, e.g. 127.0.0.1:8080")?;

        // 使用 Ok 包装配置结构体返回
        Ok(Self {
            database_url,
            bind_addr,
        })
    }
}

// ============================================================
// 单元测试模块
// ============================================================
// #[cfg(test)] 只在运行测试时编译此模块
// Rust 鼓励编写单元测试，测试代码与被测代码放在一起
#[cfg(test)]
mod tests {
    // 导入父模块的 AppConfig
    use super::AppConfig;
    // 导入环境变量操作模块
    use std::env;

    // ========================================================
    // EnvVarGuard 结构体
    // ========================================================
    // 这是一个 RAII (Resource Acquisition Is Initialization) 模式
    // 用于在测试时临时修改环境变量，测试结束后自动恢复
    struct EnvVarGuard {
        /// 环境变量名称
        key: &'static str,
        /// 环境变量的原始值 (可能不存在)
        prev: Option<String>,
    }

    impl EnvVarGuard {
        /// 设置环境变量的值
        ///
        /// # 参数
        /// - key: 环境变量名称
        /// - value: 要设置的值
        ///
        /// # 返回
        /// 返回一个 EnvVarGuard，在 drop 时自动恢复原值
        fn set(key: &'static str, value: &str) -> Self {
            // 保存原始值 (如果存在)
            let prev = env::var(key).ok();
            // 设置新值
            env::set_var(key, value);
            // 返回 guard 对象
            EnvVarGuard { key, prev }
        }

        /// 删除环境变量
        ///
        /// # 参数
        /// - key: 要删除的环境变量名
        fn remove(key: &'static str) -> Self {
            // 保存原始值 (如果存在)
            let prev = env::var(key).ok();
            // 删除环境变量
            env::remove_var(key);
            // 返回 guard 对象
            EnvVarGuard { key, prev }
        }
    }

    // ========================================================
    // Drop trait 实现
    // ========================================================
    // Drop trait 在对象生命周期结束时自动调用
    // 用于清理资源，类似析构函数
    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            // 恢复环境变量的原始值
            match &self.prev {
                // 如果原来有值，恢复原值
                Some(val) => env::set_var(self.key, val),
                // 如果原来不存在，删除它
                None => env::remove_var(self.key),
            }
        }
    }

    // ========================================================
    // 单元测试：配置读取测试
    // ========================================================
    #[test]
    fn config_reads_env_and_uses_default_bind_addr() {
        // 设置数据库 URL 环境变量
        // 使用 guard 确保测试后自动清理
        let _db_guard = EnvVarGuard::set(
            "QB_DATABASE_URL",
            "postgres://postgres:postgres@localhost/qb",
        );
        // 删除绑定地址环境变量 (测试默认值)
        let _bind_guard = EnvVarGuard::remove("QB_BIND_ADDR");

        // 调用 from_env() 加载配置
        // .expect() 在失败时 panic 并显示错误信息
        let cfg = AppConfig::from_env().expect("config should load");

        // 验证默认绑定地址
        assert_eq!(cfg.bind_addr.to_string(), "127.0.0.1:8080");

        // 验证数据库 URL
        assert_eq!(
            cfg.database_url,
            "postgres://postgres:postgres@localhost/qb"
        );
    }
}

/*
 * ============================================================
 * 知识点讲解 (Rust 新手必读)
 * ============================================================
 *
 * 1. 结构体 (struct)
 *    - 用于组织相关数据
 *    - 类似其他语言的 class，但更轻量
 *    - 字段默认是私有的
 *
 * 2. impl 块
 *    - 为结构体定义方法
 *    - Self 指代结构体本身
 *    - &self 表示借用 (不获取所有权)
 *
 * 3. Result 和 ? 操作符
 *    - Result<T, E> 表示可能失败的操作
 *    - ? 自动传播错误，简化代码
 *
 * 4. 派生宏 (Derive Macros)
 *    - Debug: 支持 {:?} 格式化输出
 *    - Clone: .clone() 深拷贝
 *    - PartialEq: 支持 == 比较
 *    - Eq: 标记类型满足等价关系
 *
 * 5. RAII 模式
 *    - 资源获取即初始化
 *    - 利用 Drop trait 自动清理
 *    - Rust 特有的资源管理模式
 *
 * ============================================================
 */
