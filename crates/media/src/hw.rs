//! Hardware-accelerated decoding through FFmpeg hwaccels.
use std::{ffi::c_void, ptr};

use ffmpeg_next::ffi::*;
use ffmpeg_next::frame;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HwKind {
    VideoToolbox,
    D3d11va,
    Vaapi,
}

impl HwKind {
    pub fn for_platform() -> Option<HwKind> {
        if cfg!(target_os = "macos") {
            Some(HwKind::VideoToolbox)
        } else if cfg!(target_os = "windows") {
            Some(HwKind::D3d11va)
        } else if cfg!(target_os = "linux") {
            Some(HwKind::Vaapi)
        } else {
            None
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            HwKind::VideoToolbox => "videotoolbox",
            HwKind::D3d11va => "d3d11va",
            HwKind::Vaapi => "vaapi",
        }
    }

    fn device_type(self) -> AVHWDeviceType {
        match self {
            HwKind::VideoToolbox => AVHWDeviceType::AV_HWDEVICE_TYPE_VIDEOTOOLBOX,
            HwKind::D3d11va => AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA,
            HwKind::Vaapi => AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
        }
    }

    fn pix_fmt(self) -> AVPixelFormat {
        match self {
            HwKind::VideoToolbox => AVPixelFormat::AV_PIX_FMT_VIDEOTOOLBOX,
            HwKind::D3d11va => AVPixelFormat::AV_PIX_FMT_D3D11,
            HwKind::Vaapi => AVPixelFormat::AV_PIX_FMT_VAAPI,
        }
    }
}

/// Picks the hardware format stored in `ctx.opaque`; if the decoder does not offer it
/// for this stream (unsupported profile), falls back to the first software format.
unsafe extern "C" fn get_format(
    ctx: *mut AVCodecContext,
    fmts: *const AVPixelFormat,
) -> AVPixelFormat {
    unsafe {
        let wanted = (*ctx).opaque as isize as i32;
        let mut p = fmts;
        while *p != AVPixelFormat::AV_PIX_FMT_NONE {
            if *p as i32 == wanted {
                return *p;
            }
            p = p.add(1);
        }
        let mut p = fmts;
        while *p != AVPixelFormat::AV_PIX_FMT_NONE {
            let desc = av_pix_fmt_desc_get(*p);
            if !desc.is_null() && ((*desc).flags & (AV_PIX_FMT_FLAG_HWACCEL as u64)) == 0 {
                return *p;
            }
            p = p.add(1);
        }
        AVPixelFormat::AV_PIX_FMT_NONE
    }
}

/// Attaches a hardware device to a not-yet-opened codec context.
///
/// # Safety
/// `ctx` must be a valid, unopened `AVCodecContext`.
pub unsafe fn attach(ctx: *mut AVCodecContext, kind: HwKind) -> Result<(), String> {
    unsafe {
        let mut device: *mut AVBufferRef = ptr::null_mut();
        let ret = av_hwdevice_ctx_create(
            &mut device,
            kind.device_type(),
            ptr::null(),
            ptr::null_mut(),
            0,
        );
        if ret < 0 {
            return Err(format!(
                "av_hwdevice_ctx_create({}) failed with {ret}",
                kind.name()
            ));
        }
        // The codec context takes ownership of the reference.
        (*ctx).hw_device_ctx = device;
        (*ctx).opaque = kind.pix_fmt() as i32 as isize as *mut c_void;
        (*ctx).get_format = Some(get_format);
        Ok(())
    }
}

pub fn is_hw_frame(f: &frame::Video) -> bool {
    // SAFETY: reading a field of a valid frame.
    unsafe { !(*f.as_ptr()).hw_frames_ctx.is_null() }
}

/// Copies a GPU frame into system memory (NV12 or P010).
pub fn download(hw: &frame::Video) -> Result<frame::Video, String> {
    let mut sw = frame::Video::empty();
    // SAFETY: both frames are valid; `sw` is empty so FFmpeg allocates its buffers.
    unsafe {
        let ret = av_hwframe_transfer_data(sw.as_mut_ptr(), hw.as_ptr(), 0);
        if ret < 0 {
            return Err(format!("av_hwframe_transfer_data failed with {ret}"));
        }
        (*sw.as_mut_ptr()).pts = (*hw.as_ptr()).pts;
        (*sw.as_mut_ptr()).best_effort_timestamp = (*hw.as_ptr()).best_effort_timestamp;
    }
    Ok(sw)
}
