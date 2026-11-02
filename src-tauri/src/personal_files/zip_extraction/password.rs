//! A live-plan secret only. It cannot be cloned, serialized or debug printed.
use super::*;
pub(super) struct Password(zeroize::Zeroizing<Vec<u8>>);
impl Password {
    pub(super) fn new(value: String) -> Result<Self, String> {
        let value = zeroize::Zeroizing::new(value.into_bytes());
        if value.is_empty() || value.len() > 1024 {
            return Err("ZIP 密码须为 1–1024 字节，不会保存到历史。".into());
        }
        Ok(Self(value))
    }
    pub(super) fn bytes(&self) -> &[u8] {
        &self.0
    }
}
pub(super) fn open<'a, R: Read + Seek>(
    zip: &'a mut zip::ZipArchive<R>,
    index: usize,
    password: Option<&Password>,
) -> Result<zip::read::ZipFile<'a, R>, String> {
    let result = match password {
        Some(p) => zip.by_index_decrypt(index, p.bytes()),
        None => zip.by_index(index),
    };
    result.map_err(|e| match e {
        zip::result::ZipError::InvalidPassword => {
            "ZIP 密码不正确或无法验证，请重新检查；未解压。".into()
        }
        zip::result::ZipError::UnsupportedArchive(zip::result::ZipError::PASSWORD_REQUIRED) => {
            "此 ZIP 需要密码，请输入后重新检查。".into()
        }
        _ => "ZIP 加密、压缩方法或条目结构无法读取，未解压。".into(),
    })
}
#[cfg(test)]
mod tests;
