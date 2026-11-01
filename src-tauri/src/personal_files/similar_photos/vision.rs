//! Fixed on-device Vision revision. Feature objects never leave this worker.
use objc2::{
    msg_send,
    rc::{Allocated, Retained},
    runtime::{AnyClass, AnyObject, Bool},
};
use objc2_foundation::{NSArray, NSData, NSDictionary, NSError, NSString};
#[link(name = "Vision", kind = "framework")]
extern "C" {
    static VNVisionVersionNumber: f64;
}
pub(in crate::personal_files::similar_photos) struct Feature(Retained<AnyObject>);
impl Feature {
    pub(in crate::personal_files::similar_photos) fn new(png: &[u8]) -> Result<Self, String> {
        unsafe {
            let _ = VNVisionVersionNumber;
            let request_class = AnyClass::get(c"VNGenerateImageFeaturePrintRequest")
                .ok_or("系统图像特征服务不可用。")?;
            let handler_class =
                AnyClass::get(c"VNImageRequestHandler").ok_or("系统图像读取服务不可用。")?;
            let request: Retained<AnyObject> = msg_send![request_class, new];
            let _: () = msg_send![&*request,setRevision:2usize];
            let data = NSData::with_bytes(png);
            let options = NSDictionary::<NSString, AnyObject>::new();
            let allocated: Allocated<AnyObject> = msg_send![handler_class, alloc];
            let handler: Retained<AnyObject> =
                msg_send![allocated,initWithData:&*data,options:&*options];
            let requests = NSArray::from_slice(&[&*request]);
            let mut error: *mut NSError = std::ptr::null_mut();
            let ok: Bool = msg_send![&*handler,performRequests:&*requests,error:&mut error];
            if !ok.as_bool() {
                return Err("系统未完成图像特征计算，未核对的照片保留。".into());
            }
            let results: Option<Retained<NSArray<AnyObject>>> = msg_send![&*request, results];
            let result = results
                .and_then(|a| a.firstObject())
                .ok_or("系统未返回可比较的图像特征。")?;
            Ok(Self(result))
        }
    }
    pub(in crate::personal_files::similar_photos) fn distance(
        &self,
        other: &Self,
    ) -> Result<f32, String> {
        unsafe {
            let mut distance = 0f32;
            let mut error: *mut NSError = std::ptr::null_mut();
            let ok: Bool = msg_send![&*self.0,computeDistance:&mut distance,toFeaturePrintObservation:&*other.0,error:&mut error];
            if !ok.as_bool() || !distance.is_finite() || distance < 0. {
                return Err("图像特征距离无法核对，未组成相似分组。".into());
            }
            Ok(distance)
        }
    }
}
