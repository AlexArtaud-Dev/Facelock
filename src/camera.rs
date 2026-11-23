//! Capture webcam via Media Foundation (aucune dépendance externe).
//! Lecture synchrone à la demande : aucune image n'est traitée entre deux
//! vérifications, et la caméra est totalement libérée (LED éteinte) au drop.

use crate::engine::Frame;
use windows::core::{Interface, GUID, PWSTR};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_MULTITHREADED};

const FIRST_VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
const ALL_STREAMS: u32 = MF_SOURCE_READER_ALL_STREAMS.0 as u32;

/// À appeler une fois par thread utilisant la caméra.
pub fn init_thread() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let _ = MFStartup(MF_VERSION, MFSTARTUP_LITE);
    }
}

fn devices() -> windows::core::Result<Vec<IMFActivate>> {
    unsafe {
        let mut attrs = None;
        MFCreateAttributes(&mut attrs, 1)?;
        let attrs = attrs.unwrap();
        attrs.SetGUID(&MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID)?;
        let mut ptr: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count = 0u32;
        MFEnumDeviceSources(&attrs, &mut ptr, &mut count)?;
        let mut out = Vec::with_capacity(count as usize);
        if !ptr.is_null() {
            for i in 0..count as usize {
                if let Some(a) = (*ptr.add(i)).take() {
                    out.push(a);
                }
            }
            CoTaskMemFree(Some(ptr as *const _));
        }
        Ok(out)
    }
}

fn friendly_name(a: &IMFActivate) -> String {
    unsafe {
        let mut p = PWSTR::null();
        let mut len = 0u32;
        if a.GetAllocatedString(&MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, &mut p, &mut len).is_ok() {
            let s = p.to_string().unwrap_or_default();
            CoTaskMemFree(Some(p.0 as *const _));
            s
        } else {
            String::from("Caméra")
        }
    }
}

/// Noms des webcams, dans l'ordre des index.
pub fn list_cameras() -> Vec<String> {
    devices().map(|d| d.iter().map(friendly_name).collect()).unwrap_or_default()
}

pub struct Camera {
    source: IMFMediaSource,
    reader: IMFSourceReader,
    pub w: usize,
    pub h: usize,
    stride: i32,
    pub name: String,
}

impl Camera {
    /// Ouvre la caméra `name` si elle existe, sinon l'index donné.
    pub fn open(index: usize, name: &str, want: [u32; 2]) -> windows::core::Result<Self> {
        let devs = devices()?;
        let pos = devs
            .iter()
            .position(|d| !name.is_empty() && friendly_name(d) == name)
            .unwrap_or(index);
        let act = devs.get(pos).ok_or_else(|| windows::core::Error::from(MF_E_NOT_FOUND))?;
        let cam_name = friendly_name(act);
        unsafe {
            let source: IMFMediaSource = act.ActivateObject()?;
            let mut attrs = None;
            MFCreateAttributes(&mut attrs, 2)?;
            let attrs = attrs.unwrap();
            attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)?;
            let reader = match MFCreateSourceReaderFromMediaSource(&source, &attrs) {
                Ok(r) => r,
                Err(e) => {
                    let _ = source.Shutdown();
                    return Err(e);
                }
            };
            let mut cam = Camera { source, reader, w: 0, h: 0, stride: 0, name: cam_name };
            cam.configure(want)?;
            Ok(cam)
        }
    }

    unsafe fn configure(&mut self, want: [u32; 2]) -> windows::core::Result<()> {
        let r = &self.reader;
        let _ = r.SetStreamSelection(ALL_STREAMS, false);
        r.SetStreamSelection(FIRST_VIDEO, true)?;

        // Format natif le plus proche de la résolution voulue ; à taille égale,
        // on préfère un format non compressé (pas de décodage MJPEG).
        let mut best: Option<(i64, IMFMediaType)> = None;
        let mut i = 0;
        while let Ok(mt) = r.GetNativeMediaType(FIRST_VIDEO, i) {
            i += 1;
            let Ok(size) = mt.GetUINT64(&MF_MT_FRAME_SIZE) else { continue };
            let (w, h) = ((size >> 32) as i64, (size & 0xFFFF_FFFF) as i64);
            let sub = mt.GetGUID(&MF_MT_SUBTYPE).unwrap_or(GUID::zeroed());
            let fps = mt
                .GetUINT64(&MF_MT_FRAME_RATE)
                .map(|v| (v >> 32) as f64 / ((v & 0xFFFF_FFFF).max(1)) as f64)
                .unwrap_or(30.0);
            let mut cost = (w - want[0] as i64).abs() + (h - want[1] as i64).abs();
            cost = cost * 10 + if sub == MFVideoFormat_MJPG { 5 } else { 0 };
            if fps < 10.0 {
                cost += 10_000;
            }
            if best.as_ref().is_none_or(|(c, _)| cost < *c) {
                best = Some((cost, mt));
            }
        }
        if let Some((_, mt)) = best {
            let _ = r.SetCurrentMediaType(FIRST_VIDEO, None, &mt);
        }

        let out = MFCreateMediaType()?;
        out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        out.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
        r.SetCurrentMediaType(FIRST_VIDEO, None, &out)?;

        let cur = r.GetCurrentMediaType(FIRST_VIDEO)?;
        let size = cur.GetUINT64(&MF_MT_FRAME_SIZE)?;
        self.w = (size >> 32) as usize;
        self.h = (size & 0xFFFF_FFFF) as usize;
        self.stride = cur
            .GetUINT32(&MF_MT_DEFAULT_STRIDE)
            .map(|s| s as i32)
            .unwrap_or((self.w * 4) as i32);
        Ok(())
    }

    /// Lit une image. `fresh` vide d'abord la file interne pour obtenir une
    /// image prise maintenant et non il y a plusieurs secondes.
    pub fn read(&mut self, fresh: bool) -> windows::core::Result<Frame> {
        unsafe {
            if fresh {
                let _ = self.reader.Flush(FIRST_VIDEO);
            }
            for _ in 0..50 {
                let mut flags = 0u32;
                let mut sample: Option<IMFSample> = None;
                self.reader.ReadSample(FIRST_VIDEO, 0, None, Some(&mut flags), None, Some(&mut sample))?;
                if flags & (MF_SOURCE_READERF_ERROR.0 as u32 | MF_SOURCE_READERF_ENDOFSTREAM.0 as u32) != 0 {
                    return Err(windows::core::Error::from(MF_E_INVALIDREQUEST));
                }
                if flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0 {
                    if let Ok(cur) = self.reader.GetCurrentMediaType(FIRST_VIDEO) {
                        if let Ok(size) = cur.GetUINT64(&MF_MT_FRAME_SIZE) {
                            self.w = (size >> 32) as usize;
                            self.h = (size & 0xFFFF_FFFF) as usize;
                        }
                    }
                }
                if let Some(s) = sample {
                    return self.copy_sample(&s);
                }
            }
            Err(windows::core::Error::from(MF_E_INVALIDREQUEST))
        }
    }

    unsafe fn copy_sample(&self, s: &IMFSample) -> windows::core::Result<Frame> {
        let (w, h) = (self.w, self.h);
        let row = w * 4;
        let mut bgra = vec![0u8; row * h];
        let buf = s.ConvertToContiguousBuffer()?;
        if let Ok(b2) = buf.cast::<IMF2DBuffer>() {
            let mut p = std::ptr::null_mut();
            let mut pitch = 0i32;
            b2.Lock2D(&mut p, &mut pitch)?;
            for y in 0..h {
                let src = p.offset(y as isize * pitch as isize);
                std::ptr::copy_nonoverlapping(src, bgra.as_mut_ptr().add(y * row), row);
            }
            let _ = b2.Unlock2D();
        } else {
            let mut p = std::ptr::null_mut();
            let mut len = 0u32;
            buf.Lock(&mut p, None, Some(&mut len))?;
            let stride = self.stride.unsigned_abs() as usize;
            if (len as usize) >= stride * h {
                for y in 0..h {
                    let sy = if self.stride < 0 { h - 1 - y } else { y };
                    std::ptr::copy_nonoverlapping(p.add(sy * stride), bgra.as_mut_ptr().add(y * row), row);
                }
            }
            let _ = buf.Unlock();
        }
        Ok(Frame { w, h, bgra })
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        unsafe {
            let _ = self.source.Shutdown();
        }
    }
}
