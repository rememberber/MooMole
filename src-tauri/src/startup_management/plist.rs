use super::*;
use objc2::{rc::autoreleasepool, runtime::AnyObject};
use objc2_foundation::{
    NSArray, NSData, NSDictionary, NSPropertyListReadOptions, NSPropertyListSerialization, NSString,
};
pub(super) fn text(value: &AnyObject) -> Result<String, String> {
    let text = value
        .downcast_ref::<NSString>()
        .ok_or("登录配置字段类型不支持。")?
        .to_string();
    if text.len() > 4096 || text.chars().any(char::is_control) {
        return Err("登录配置文本超出支持范围。".into());
    }
    Ok(text)
}
pub(super) fn args(value: &AnyObject) -> Result<Vec<String>, String> {
    let a = value
        .downcast_ref::<NSArray>()
        .ok_or("程序参数不是结构化数组。")?;
    if a.count() == 0 || a.count() > 128 {
        return Err("启动参数数量不受支持。".into());
    }
    let values = a.iter().map(|v| text(&v)).collect::<Result<Vec<_>, _>>()?;
    if values.iter().map(|s| s.len()).sum::<usize>() > 32768 {
        return Err("启动参数超过读取范围。".into());
    }
    Ok(values)
}
pub(super) fn get(d: &NSDictionary, key: &str) -> Option<objc2::rc::Retained<AnyObject>> {
    d.objectForKey(&NSString::from_str(key))
}
pub(super) fn parse(bytes: &[u8]) -> Result<Definition, String> {
    autoreleasepool(|_| {
        let value = unsafe {
            NSPropertyListSerialization::propertyListWithData_options_format_error(
                &NSData::with_bytes(bytes),
                NSPropertyListReadOptions::Immutable,
                std::ptr::null_mut(),
            )
        }
        .map_err(|_| "登录配置格式无法识别。")?;
        let d = value
            .downcast_ref::<NSDictionary>()
            .ok_or("登录配置不是字典。")?;
        if d.count() > 128
            || [
                "BundleProgram",
                "RootDirectory",
                "UserName",
                "GroupName",
                "SMPrivilegedExecutables",
            ]
            .iter()
            .any(|k| get(d, k).is_some())
        {
            return Err("此登录配置需要其他管理来源，本轮保留。".into());
        }
        let label = text(&*get(d, "Label").ok_or("登录配置缺少标签。")?)?;
        if label.is_empty()
            || label.len() > 200
            || !label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err("登录标签不受支持。".into());
        }
        let mut arguments = get(d, "ProgramArguments")
            .map(|v| args(&v))
            .transpose()?
            .unwrap_or_default();
        let program = get(d, "Program")
            .map(|v| text(&v))
            .transpose()?
            .or_else(|| arguments.first().cloned())
            .ok_or("缺少确切程序位置。")?;
        let path = Path::new(&program);
        if !path.is_absolute()
            || !path.components().all(|c| {
                matches!(
                    c,
                    std::path::Component::RootDir | std::path::Component::Normal(_)
                )
            })
        {
            return Err("启动程序使用相对位置或别名语义，本轮保留。".into());
        }
        if arguments.is_empty() {
            arguments.push(program.clone());
        }
        Ok(Definition {
            label,
            program,
            arguments,
        })
    })
}
