//! Optional read-only artwork. No file URL, arbitrary path IPC, app launch or writes.
use std::path::{Component, Path};

use std::{sync::Arc, time::Duration};
use tokio::sync::Semaphore;

pub async fn load(path: String, slots: Arc<Semaphore>) -> Option<String> {
    run(slots, move || render(&path), Duration::from_secs(3)).await
}
async fn run(
    slots: Arc<Semaphore>,
    render: impl FnOnce() -> Option<String> + Send + 'static,
    wait: Duration,
) -> Option<String> {
    let slot = slots.try_acquire_owned().ok()?;
    let worker = tokio::task::spawn_blocking(move || {
        let _slot = slot;
        render()
    });
    tokio::time::timeout(wait, worker).await.ok()?.ok()?
}

pub fn render(raw: &str) -> Option<String> {
    if !valid_path(raw) {
        return None;
    }
    #[cfg(target_os = "macos")]
    {
        mac::render(Path::new(raw))
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}
fn valid_path(raw: &str) -> bool {
    let path = Path::new(raw);
    !raw.is_empty()
        && raw.len() <= 4096
        && !raw.contains('\0')
        && path.is_absolute()
        && path.extension().is_some_and(|s| s == "app")
        && path
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use objc2::{rc::autoreleasepool, AnyThread};
    use objc2_app_kit::{
        NSBitmapImageFileType, NSBitmapImageRep, NSCompositingOperation, NSDeviceRGBColorSpace,
        NSGraphicsContext, NSWorkspace,
    };
    use objc2_foundation::{
        NSDataBase64EncodingOptions, NSDictionary, NSPoint, NSRect, NSSize, NSString,
    };
    use std::os::macos::fs::MetadataExt as DarwinMetadataExt;
    use std::{
        fs::{File, Metadata, OpenOptions},
        os::{
            fd::AsRawFd,
            unix::fs::{MetadataExt, OpenOptionsExt},
        },
    };

    // sys/stat.h: absent from the pinned libc bindings.
    const SF_DATALESS: u32 = 0x40000000;

    fn identity(m: &Metadata) -> (u64, u64, i64, i64) {
        (m.dev(), m.ino(), m.ctime(), m.ctime_nsec())
    }
    fn open_local(path: &Path) -> Option<File> {
        // Reject aliases/link ancestors rather than following to a different object.
        if path.canonicalize().ok()? != path {
            return None;
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .ok()?;
        let meta = file.metadata().ok()?;
        if !meta.is_dir() || meta.st_flags() & SF_DATALESS != 0 {
            return None;
        }
        let mut volume = std::mem::MaybeUninit::<libc::statfs>::zeroed();
        if unsafe { libc::fstatfs(file.as_raw_fd(), volume.as_mut_ptr()) } != 0 {
            return None;
        }
        if unsafe { volume.assume_init() }.f_flags & libc::MNT_LOCAL as u32 == 0 {
            return None;
        }
        Some(file)
    }
    struct Graphics;
    impl Drop for Graphics {
        fn drop(&mut self) {
            NSGraphicsContext::restoreGraphicsState_class();
        }
    }

    pub(super) fn render(path: &Path) -> Option<String> {
        let held = open_local(path)?;
        let before = held.metadata().ok()?;
        let result = autoreleasepool(|_| {
            // Draw into one independent 64px RGBA bitmap; never serialize the
            // multi-resolution source image or its potentially large TIFF data.
            let bitmap = unsafe {
                NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                NSBitmapImageRep::alloc(), std::ptr::null_mut(), 64, 64, 8, 4, true, false, NSDeviceRGBColorSpace, 256, 32
            )
            }?;
            let data = bitmap.bitmapData();
            if data.is_null() || bitmap.bytesPerRow() != 256 || bitmap.pixelsHigh() != 64 {
                return None;
            }
            // With these explicit planar/stride parameters the allocation is 16KiB.
            unsafe {
                std::ptr::write_bytes(data, 0, 64 * 256);
            }
            let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&bitmap)?;
            let image =
                NSWorkspace::sharedWorkspace().iconForFile(&NSString::from_str(path.to_str()?));
            NSGraphicsContext::saveGraphicsState_class();
            let _graphics = Graphics;
            NSGraphicsContext::setCurrentContext(Some(&context));
            image.drawInRect_fromRect_operation_fraction(
                NSRect::new(NSPoint::new(0., 0.), NSSize::new(64., 64.)),
                NSRect::ZERO,
                NSCompositingOperation::Copy,
                1.,
            );
            let png = unsafe {
                bitmap.representationUsingType_properties(
                    NSBitmapImageFileType::PNG,
                    &NSDictionary::new(),
                )
            }?;
            if png.length() == 0 || png.length() > 24 * 1024 {
                return None;
            }
            Some(format!(
                "data:image/png;base64,{}",
                png.base64EncodedStringWithOptions(NSDataBase64EncodingOptions::empty())
            ))
        })?;
        let after = std::fs::symlink_metadata(path).ok()?;
        if !after.is_dir()
            || identity(&after) != identity(&before)
            || identity(&held.metadata().ok()?) != identity(&before)
        {
            return None;
        }
        Some(result)
    }

    #[test]
    fn native_bitmap_is_small_png_and_invalid_targets_fall_back() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let app = root.join("图标 空格.app");
        std::fs::create_dir(&app).unwrap();
        let png = super::render(app.to_str().unwrap()).unwrap();
        assert!(png.starts_with("data:image/png;base64,iVBORw0KGgo"));
        assert!(png.len() <= 24 * 1024 * 4 / 3 + 22);
        let decoded = objc2_foundation::NSData::initWithBase64EncodedString_options(
            objc2_foundation::NSData::alloc(),
            &NSString::from_str(png.strip_prefix("data:image/png;base64,").unwrap()),
            objc2_foundation::NSDataBase64DecodingOptions::empty(),
        )
        .unwrap()
        .to_vec();
        assert_eq!(&decoded[12..16], b"IHDR");
        assert_eq!(u32::from_be_bytes(decoded[16..20].try_into().unwrap()), 64);
        assert_eq!(u32::from_be_bytes(decoded[20..24].try_into().unwrap()), 64);
        let link = root.join("link.app");
        std::os::unix::fs::symlink(&app, &link).unwrap();
        assert!(super::render(link.to_str().unwrap()).is_none());
        let file = root.join("file.app");
        std::fs::write(&file, b"not an app").unwrap();
        assert!(super::render(file.to_str().unwrap()).is_none());
        assert!(super::render(root.join("missing.app").to_str().unwrap()).is_none());
    }
}
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn timeout_retains_the_actual_worker_slot_and_rejects_extra_reads() {
        let slots = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
        let (started, observing) = tokio::sync::oneshot::channel();
        let (release, waiting) = std::sync::mpsc::channel();
        let owned = slots.clone();
        let pending = tokio::spawn(async move {
            super::run(
                owned,
                move || {
                    started.send(()).unwrap();
                    waiting
                        .recv_timeout(std::time::Duration::from_secs(2))
                        .unwrap();
                    Some("ignored after timeout".into())
                },
                std::time::Duration::from_millis(10),
            )
            .await
        });
        observing.await.unwrap();
        assert!(pending.await.unwrap().is_none());
        assert_eq!(slots.available_permits(), 0);
        assert!(super::run(
            slots.clone(),
            || panic!("no extra worker"),
            std::time::Duration::from_millis(10)
        )
        .await
        .is_none());
        release.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while slots.available_permits() == 0 {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            super::run(
                slots,
                || Some("fresh".into()),
                std::time::Duration::from_secs(1)
            )
            .await
            .as_deref(),
            Some("fresh")
        );
    }

    #[test]
    fn paths_must_be_bounded_absolute_app_names_and_never_commands_or_urls() {
        for path in [
            "",
            "relative.app",
            "/a/../b.app",
            "https://example/a.app",
            "/tmp/x.txt",
            "/a\0.app",
        ] {
            assert!(!super::valid_path(path), "{path:?}");
        }
        assert!(!super::valid_path(&format!("/{}.app", "a".repeat(4096))));
        assert!(super::valid_path("/Applications/中文\n#$(literal).app"));
    }
}
