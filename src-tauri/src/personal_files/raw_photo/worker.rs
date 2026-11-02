use super::*;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Input {
    pub id: String,
    pub sha256: String,
    pub bytes: u64,
    pub device: String,
    pub inode: String,
    pub max: u32,
    pub extension: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::personal_files) struct Output {
    pub id: String,
    pub sha256: String,
    pub width: u64,
    pub height: u64,
    pub type_identifier: String,
    pub png: String,
}
impl Output {
    pub(super) fn validate(&self, input: &Input) -> Result<(), String> {
        if self.id != input.id
            || self.sha256 != input.sha256
            || self.width == 0
            || self.height == 0
            || self.width > 60000
            || self.height > 60000
            || self
                .width
                .checked_mul(self.height)
                .is_none_or(|p| p > 80_000_000)
            || self.type_identifier.is_empty()
            || self.type_identifier.len() > 256
            || !self
                .type_identifier
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
            || self.png.len() > 5_592_432
            || !self.png.starts_with("data:image/png;base64,iVBOR")
        {
            return Err("RAW 解码结果与来源或尺寸不符。".into());
        }
        Ok(())
    }
}
pub(super) fn decode_file(input: Input, mut file: File) -> Result<Output, String> {
    if uuid::Uuid::parse_str(&input.id).is_err()
        || input.bytes == 0
        || input.bytes > FILE_LIMIT
        || ![384, 1024].contains(&input.max)
        || input.extension.len() > 4
        || !input.extension.bytes().all(|b| b.is_ascii_alphanumeric())
        || !supported(Path::new(&format!("photo.{}", input.extension)))
        || input.sha256.len() != 64
        || !input
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("RAW 解码请求超出支持范围。".into());
    }
    let m = file.metadata().map_err(|e| e.to_string())?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.nlink() != 0
        || m.len() != input.bytes
        || m.dev().to_string() != input.device
        || m.ino().to_string() != input.inode
    {
        return Err("RAW 匿名副本身份无法核对。".into());
    }
    file.rewind().map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(FILE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 != input.bytes || format!("{:x}", Sha256::digest(&bytes)) != input.sha256
    {
        return Err("RAW 匿名副本内容无法核对。".into());
    }
    let (raster, kind) = media::Raster::decode_raw(&bytes, input.max, &input.extension)?;
    let out = Output {
        id: input.id.clone(),
        sha256: input.sha256.clone(),
        width: raster.width,
        height: raster.height,
        type_identifier: kind,
        png: raster.png()?,
    };
    out.validate(&input)?;
    Ok(out)
}
pub(crate) fn run() {
    // A codec may wait without consuming CPU. This owned process also exits
    // after a bounded wait if its supervising GUI disappears unexpectedly.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(20));
        std::process::exit(3);
    });
    // Limits apply only to this owned worker, never the GUI or user applications.
    unsafe {
        let cpu = libc::rlimit {
            rlim_cur: 12,
            rlim_max: 12,
        };
        libc::setrlimit(libc::RLIMIT_CPU, &cpu);
        let core = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        libc::setrlimit(libc::RLIMIT_CORE, &core);
    }
    let result = (|| -> Result<Vec<u8>, String> {
        let mut b = vec![];
        std::io::stdin()
            .take(8193)
            .read_to_end(&mut b)
            .map_err(|e| e.to_string())?;
        if b.len() > 8192 {
            return Err("RAW 请求过大。".into());
        }
        let input = serde_json::from_slice(&b).map_err(|_| "RAW 请求无法读取。")?;
        if unsafe { libc::fcntl(FD, libc::F_GETFD) } < 0 {
            return Err("RAW 句柄不存在。".into());
        }
        let file = unsafe { File::from_raw_fd(FD) };
        let output = objc2::rc::autoreleasepool(|_| decode_file(input, file))?;
        serde_json::to_vec(&output).map_err(|e| e.to_string())
    })();
    match result {
        Ok(bytes) if bytes.len() <= 6 * 1024 * 1024 => {
            if std::io::stdout().write_all(&bytes).is_err() {
                std::process::exit(2)
            }
        }
        Ok(_) => std::process::exit(2),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1)
        }
    }
}
