//! One isolated native decoder process per selected, validated private snapshot.
use super::*;
use objc2::{
    encode::{Encode, Encoding, RefEncode},
    msg_send,
    rc::{Allocated, Retained},
    runtime::{AnyClass, AnyObject, Bool},
};
use objc2_foundation::{NSArray, NSDictionary, NSError, NSSize, NSString, NSURL};
use std::ffi::c_void;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Input {
    pub path: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Output {
    pub duration_ms: u64,
    pub width: u64,
    pub height: u64,
    pub frames: Vec<Frame>,
}
impl Output {
    pub(super) fn validate(&self) -> Result<(), String> {
        if !(2000..=1_800_000).contains(&self.duration_ms)
            || self.width == 0
            || self.height == 0
            || self.width > 384
            || self.height > 384
            || self.frames.len() != 3
            || self.frames.iter().enumerate().any(|(i, f)| {
                f.at_ms.abs_diff(self.duration_ms * [1, 5, 9][i] / 10) > 250
                    || f.thumbnail.len() > 800_000
                    || !f.thumbnail.starts_with("data:image/png;base64,iVBOR")
            })
        {
            return Err("视频采样时长、画面或位置无法核对。".into());
        }
        Ok(())
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Time {
    value: i64,
    timescale: i32,
    flags: u32,
    epoch: i64,
}
unsafe impl Encode for Time {
    const ENCODING: Encoding = Encoding::Struct(
        "?",
        &[
            Encoding::LongLong,
            Encoding::Int,
            Encoding::UInt,
            Encoding::LongLong,
        ],
    );
}
unsafe impl RefEncode for Time {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Self::ENCODING);
}
impl Time {
    fn ms(value: u64) -> Self {
        Self {
            value: value as i64,
            timescale: 1000,
            flags: 1,
            epoch: 0,
        }
    }
    fn milliseconds(self) -> Result<u64, String> {
        if self.flags & 1 == 0
            || self.flags & !3 != 0
            || self.epoch != 0
            || self.timescale <= 0
            || self.value < 0
        {
            return Err("视频时间不是已支持的有限时间。".into());
        }
        let ms = (self.value as f64 * 1000. / self.timescale as f64).round();
        if !ms.is_finite() || !(0.0..=1_800_000.0).contains(&ms) {
            return Err("视频时长超过本轮三十分钟范围。".into());
        }
        Ok(ms as u64)
    }
}
#[link(name = "AVFoundation", kind = "framework")]
extern "C" {
    static AVURLAssetReferenceRestrictionsKey: *const NSString;
    static AVURLAssetPreferPreciseDurationAndTimingKey: *const NSString;
    static AVMediaTypeVideo: *const NSString;
}
fn extract(input: Input) -> Result<Output, String> {
    if input.path.len() > 4096
        || !Path::new(&input.path).is_absolute()
        || !input.path.ends_with("/video.mp4")
    {
        return Err("视频临时副本路径无法核对。".into());
    }
    let held = AnchoredPath::open(Path::new(&input.path))?;
    let meta = held.target().metadata().map_err(|e| e.to_string())?;
    if !meta.is_file()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.nlink() != 1
        || meta.len() > FILE_LIMIT
        || !held.local_materialized()
    {
        return Err("视频临时副本不是普通本地文件。".into());
    }
    preview::stream::containers::self_contained(held.target(), meta.len())?;
    unsafe {
        let asset_class = AnyClass::get(c"AVURLAsset").ok_or("系统视频采样服务不可用。")?;
        let generator_class =
            AnyClass::get(c"AVAssetImageGenerator").ok_or("系统视频采样服务不可用。")?;
        let number_class = AnyClass::get(c"NSNumber").ok_or("系统视频采样服务不可用。")?;
        let forbid: Retained<AnyObject> =
            msg_send![number_class,numberWithUnsignedInteger:0xFFFFusize];
        let precise: Retained<AnyObject> = msg_send![number_class,numberWithBool:Bool::YES];
        let options = NSDictionary::<NSString, AnyObject>::from_slices(
            &[
                &*AVURLAssetReferenceRestrictionsKey,
                &*AVURLAssetPreferPreciseDurationAndTimingKey,
            ],
            &[&*forbid, &*precise],
        );
        let url = NSURL::fileURLWithPath(&NSString::from_str(&input.path));
        let allocated: Allocated<AnyObject> = msg_send![asset_class, alloc];
        let asset: Retained<AnyObject> = msg_send![allocated,initWithURL:&*url,options:&*options];
        let protected: Bool = msg_send![&*asset, hasProtectedContent];
        let playable: Bool = msg_send![&*asset, isPlayable];
        let tracks: Retained<NSArray<AnyObject>> =
            msg_send![&*asset,tracksWithMediaType:&*AVMediaTypeVideo];
        if protected.as_bool() || !playable.as_bool() || tracks.len() != 1 {
            return Err("视频加密、不可播放或视频轨道数量未支持。".into());
        }
        let duration: Time = msg_send![&*asset, duration];
        let duration_ms = duration.milliseconds()?;
        if duration_ms < 2000 {
            return Err("本轮视频采样支持两秒至三十分钟的普通视频。".into());
        }
        let generator: Retained<AnyObject> =
            msg_send![generator_class,assetImageGeneratorWithAsset:&*asset];
        let _: () = msg_send![&*generator,setAppliesPreferredTrackTransform:Bool::YES];
        let _: () = msg_send![&*generator,setMaximumSize:NSSize::new(384.,384.)];
        let _: () = msg_send![&*generator,setRequestedTimeToleranceBefore:Time::ms(100)];
        let _: () = msg_send![&*generator,setRequestedTimeToleranceAfter:Time::ms(100)];
        let mut frames = vec![];
        let mut width = 0;
        let mut height = 0;
        for fraction in [1, 5, 9] {
            let mut actual = Time::ms(0);
            let mut error: *mut NSError = std::ptr::null_mut();
            let raw: *const c_void = msg_send![&*generator,copyCGImageAtTime:Time::ms(duration_ms*fraction/10),actualTime:&mut actual,error:&mut error];
            let raster = media::Raster::from_video_frame(raw)?;
            if width != 0 && (width, height) != (raster.width, raster.height) {
                return Err("视频采样尺寸在画面间变化，本轮保留。".into());
            }
            width = raster.width;
            height = raster.height;
            frames.push(Frame {
                at_ms: actual.milliseconds()?,
                thumbnail: raster.png()?,
            });
        }
        held.validate()?;
        let output = Output {
            duration_ms,
            width,
            height,
            frames,
        };
        output.validate()?;
        Ok(output)
    }
}
pub(crate) fn run() {
    let result = (|| -> Result<Vec<u8>, String> {
        let mut bytes = vec![];
        std::io::stdin()
            .take(8193)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 8192 {
            return Err("视频采样请求超过范围。".into());
        }
        let input = serde_json::from_slice(&bytes).map_err(|_| "视频采样请求格式无法核对。")?;
        let output = objc2::rc::autoreleasepool(|_| extract(input))?;
        let bytes = serde_json::to_vec(&output).map_err(|e| e.to_string())?;
        if bytes.len() > 2 * 1024 * 1024 {
            return Err("视频采样输出超过本轮范围。".into());
        }
        Ok(bytes)
    })();
    match result {
        Ok(bytes) => {
            if std::io::stdout().write_all(&bytes).is_err() {
                std::process::exit(2);
            }
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
