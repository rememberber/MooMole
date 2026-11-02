use super::*;
use std::{
    ffi::CString,
    io::{Read, Write},
    os::unix::fs::{MetadataExt, PermissionsExt},
};
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Copy {
    path: String,
    sha256: String,
    bytes: u64,
    witness: Witness,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Input {
    id: String,
    root: Witness,
    photo: Copy,
    movie: Copy,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Output {
    id: String,
    photo_sha256: String,
    movie_sha256: String,
    valid: bool,
    width: u32,
    height: u32,
}
unsafe extern "C" {
    fn moomole_live_photo_validate(
        photo: *const libc::c_char,
        movie: *const libc::c_char,
        width: *mut u32,
        height: *mut u32,
    ) -> libc::c_int;
}
fn copy(path: &Path, digest: String) -> Result<Copy, String> {
    let h = AnchoredPath::open(path)?;
    let m = h.target().metadata().map_err(|e| e.to_string())?;
    Ok(Copy {
        path: path.to_str().ok_or("Live Photo 临时路径无法编码。")?.into(),
        sha256: digest,
        bytes: m.len(),
        witness: h.witness,
    })
}
fn verify(c: &Copy, root: &Path, photo: bool) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    let path = Path::new(&c.path);
    if path.parent() != Some(root)
        || !hash(&c.sha256)
        || c.bytes == 0
        || c.bytes
            > if photo {
                32 * 1024 * 1024
            } else {
                128 * 1024 * 1024
            }
        || if photo {
            !is_photo(path)
        } else {
            path.file_name().is_none_or(|p| p != "motion.mov")
        }
    {
        return Err("Live Photo 临时输入范围无法核对。".into());
    }
    let held = AnchoredPath::open(path)?;
    let m = held.target().metadata().map_err(|e| e.to_string())?;
    if held.witness != c.witness
        || !held.local_materialized()
        || !m.is_file()
        || m.nlink() != 1
        || m.uid() != unsafe { libc::geteuid() }
        || m.len() != c.bytes
    {
        return Err("Live Photo 临时输入身份变化。".into());
    }
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| e.to_string())?;
    let n = f.metadata().map_err(|e| e.to_string())?;
    if (n.dev(), n.ino()) != (m.dev(), m.ino()) {
        return Err("Live Photo 输入替换。".into());
    }
    let mut digest = Sha256::new();
    let mut bytes = 0;
    let mut b = [0; 128 * 1024];
    loop {
        let n = f.read(&mut b).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        bytes += n as u64;
        if bytes > c.bytes {
            return Err("Live Photo 输入超限。".into());
        }
        digest.update(&b[..n]);
    }
    held.validate()?;
    if bytes != c.bytes || format!("{:x}", digest.finalize()) != c.sha256 {
        return Err("Live Photo 输入内容变化。".into());
    }
    Ok(())
}
fn decode(i: &Input) -> Result<Output, String> {
    let root = &i.root.nodes.last().ok_or("Live Photo 临时根缺失。")?.path;
    let t = std::env::temp_dir()
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let h = AnchoredPath::open(root)?;
    let m = h.target().metadata().map_err(|e| e.to_string())?;
    if uuid::Uuid::parse_str(&i.id).is_err()
        || root.parent() != Some(t.as_path())
        || root
            .file_name()
            .and_then(|n| n.to_str())
            .is_none_or(|n| !n.starts_with("MooMole-live-"))
        || h.witness != i.root
        || !m.is_dir()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o077 != 0
    {
        return Err("Live Photo 请求不属于受控系统临时目录。".into());
    }
    verify(&i.photo, root, true)?;
    verify(&i.movie, root, false)?;
    let photo = CString::new(i.photo.path.as_str()).map_err(|e| e.to_string())?;
    let movie = CString::new(i.movie.path.as_str()).map_err(|e| e.to_string())?;
    let mut width = 0;
    let mut height = 0;
    let result = unsafe {
        moomole_live_photo_validate(photo.as_ptr(), movie.as_ptr(), &mut width, &mut height)
    };
    if result < 0 {
        return Err("系统 Live Photo 配对未在期限内完成，保留。".into());
    }
    verify(&i.photo, root, true)?;
    verify(&i.movie, root, false)?;
    h.validate()?;
    Ok(Output {
        id: i.id.clone(),
        photo_sha256: i.photo.sha256.clone(),
        movie_sha256: i.movie.sha256.clone(),
        valid: result == 1,
        width,
        height,
    })
}
pub(crate) fn run() {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(20));
        std::process::exit(3)
    });
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
    let result = (|| -> Result<Output, String> {
        let mut b = vec![];
        std::io::stdin()
            .take(32769)
            .read_to_end(&mut b)
            .map_err(|e| e.to_string())?;
        if b.len() > 32768 {
            return Err("Live Photo 请求超限。".into());
        }
        let i: Input = serde_json::from_slice(&b).map_err(|_| "Live Photo 请求格式无效。")?;
        decode(&i)
    })();
    match result {
        Ok(out) => {
            if std::io::stdout()
                .write_all(&serde_json::to_vec(&out).unwrap())
                .is_err()
            {
                std::process::exit(2)
            }
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1)
        }
    }
}
pub(super) fn analyze(
    photo: &Item,
    movie: &Item,
    rules: &[String],
    job: Arc<Job>,
    began: Instant,
    count: &mut u64,
) -> Result<(Proof, bool), String> {
    let temp = tempfile::Builder::new()
        .prefix("MooMole-live-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .map_err(|e| e.to_string())?;
    let root = temp.path().canonicalize().map_err(|e| e.to_string())?;
    let p = root.join(format!(
        "photo.{}",
        Path::new(&photo.path)
            .extension()
            .and_then(|s| s.to_str())
            .ok_or("照片格式未知。")?
    ));
    let v = root.join("motion.mov");
    let mut image = File::create(&p).map_err(|e| e.to_string())?;
    let mut video = File::create(&v).map_err(|e| e.to_string())?;
    let a = raw_photo::content(photo, rules, &job, began, Some(&mut image), count)?;
    let b = similar_photos::video::content(movie, rules, &job, began, Some(&mut video), count)?;
    drop(image);
    drop(video);
    let input = Input {
        id: uuid::Uuid::new_v4().to_string(),
        root: AnchoredPath::open(&root)?.witness,
        photo: copy(&p, a.clone())?,
        movie: copy(&v, b.clone())?,
    };
    let mut command = tokio::process::Command::new(executable()?);
    command
        .arg("--moomole-live-photo-worker-v1")
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("TMPDIR", std::env::temp_dir())
        .current_dir("/");
    let bytes = tauri::async_runtime::block_on(crate::process::output_with_input(
        command,
        job.clone(),
        serde_json::to_vec(&input).map_err(|e| e.to_string())?,
        Duration::from_secs(120)
            .saturating_sub(began.elapsed())
            .min(Duration::from_secs(12)),
        8192,
    ))?;
    let out: Output =
        serde_json::from_slice(&bytes).map_err(|_| "Live Photo 返回格式无法核对。")?;
    if out.id != input.id
        || out.photo_sha256 != a
        || out.movie_sha256 != b
        || (out.valid
            && (out.width == 0 || out.height == 0 || out.width > 60000 || out.height > 60000))
        || job.cancelled.load(Ordering::SeqCst)
        || AnchoredPath::open(Path::new(&photo.path))?.witness != photo.witness
        || AnchoredPath::open(Path::new(&movie.path))?.witness != movie.witness
    {
        return Err("Live Photo 返回与当前来源不符。".into());
    }
    Ok((
        Proof {
            id: uuid::Uuid::new_v4().to_string(),
            photo_id: photo.id.clone(),
            movie_id: movie.id.clone(),
            photo_sha256: a,
            movie_sha256: b,
        },
        out.valid,
    ))
}
fn executable() -> Result<std::path::PathBuf, String> {
    #[cfg(test)]
    if let Some(p) = std::env::var_os("MOOMOLE_LIVE_WORKER_PATH") {
        return Ok(p.into());
    }
    std::env::current_exe().map_err(|e| e.to_string())
}
