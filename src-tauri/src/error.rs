use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("输入无效：{0}")]
    InvalidInput(String),
    #[error("网络请求失败：{0}")]
    Network(String),
    #[error("凭据存储失败：{0}")]
    Credential(String),
    #[error("配置文件操作失败：{0}")]
    Config(String),
    #[error("检测到配置文件已被其他程序修改，请重新预览后再应用")]
    ExternalModification,
    #[error("未找到服务：{0}")]
    ProviderNotFound(String),
    #[error("未找到备份：{0}")]
    BackupNotFound(String),
}

impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: serde::Serializer {
        serializer.serialize_str(&self.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self { Self::Config(value.to_string()) }
}

pub type AppResult<T> = Result<T, AppError>;
