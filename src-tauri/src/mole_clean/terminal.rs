use std::{
    fs::File,
    io::Write,
    os::fd::{AsRawFd, FromRawFd},
};
pub fn open() -> Result<(File, File), String> {
    let mut master = -1;
    let mut slave = -1;
    if unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } != 0
    {
        return Err("无法建立本次授权终端。".into());
    }
    let master = unsafe { File::from_raw_fd(master) };
    let slave = unsafe { File::from_raw_fd(slave) };
    unsafe {
        let mut settings: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(slave.as_raw_fd(), &mut settings) != 0 {
            return Err("无法核对授权终端。".into());
        }
        settings.c_lflag &= !(libc::ECHO | libc::ECHONL);
        if libc::tcsetattr(slave.as_raw_fd(), libc::TCSANOW, &settings) != 0 {
            return Err("无法关闭授权输入回显。".into());
        }
        for f in [&master, &slave] {
            libc::fcntl(f.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC);
        }
    }
    Ok((master, slave))
}
pub fn valid(password: &str) -> bool {
    !password.is_empty()
        && password.len() <= 1024
        && !password.as_bytes().iter().any(|b| matches!(b, 0 | 10 | 13))
}
pub fn submit(file: &mut File, password: String) -> Result<(), String> {
    let mut bytes = password.into_bytes();
    let result =
        if bytes.is_empty() || bytes.len() > 1024 || bytes.iter().any(|b| matches!(b, 0 | 10 | 13))
        {
            Err("授权输入无效。".into())
        } else {
            file.write_all(&bytes)
                .and_then(|_| file.write_all(b"\n"))
                .map_err(|_| "本次授权输入无法送达，请停止后重试。".into())
        };
    bytes.fill(0);
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn credentials_use_a_non_echoing_owned_terminal_and_never_child_arguments() {
        let (mut master, slave) = open().unwrap();
        let mut attributes: libc::termios = unsafe { std::mem::zeroed() };
        assert_eq!(
            unsafe { libc::tcgetattr(slave.as_raw_fd(), &mut attributes) },
            0
        );
        assert_eq!(attributes.c_lflag & (libc::ECHO | libc::ECHONL), 0);
        submit(&mut master, "owned-fixture".into()).unwrap();
        let mut command = crate::engine::command(std::path::Path::new("/bin/bash"));
        command.args(["-c","test -t 0 && test -c /dev/tty && IFS= read -r value && test \"$value\" = owned-fixture && printf verified"]);
        let job = std::sync::Arc::new(crate::engine::Job::new(crate::engine::TaskKind::MoleClean));
        let output = crate::process::output_with_terminal(
            command,
            job.clone(),
            std::time::Duration::from_secs(3),
            slave,
        )
        .await
        .unwrap();
        assert_eq!(output, b"verified");
        assert!(job.process_id().is_none());
    }
    #[test]
    fn invalid_or_multiline_credential_input_is_never_written() {
        let (mut master, _slave) = open().unwrap();
        for s in ["", "line\nsecond", "nul\0secret"] {
            assert!(submit(&mut master, s.into()).is_err());
        }
    }
}
