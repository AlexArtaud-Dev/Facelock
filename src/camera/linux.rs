//! Capture webcam Linux via V4L2 (mmap), formats YUYV ou MJPEG.

use crate::engine::Frame;
use v4l::buffer::Type;
use v4l::capability::Flags;
use v4l::io::mmap::Stream;
use v4l::io::traits::CaptureStream;
use v4l::video::Capture;
use v4l::{Device, FourCC};

pub fn init_thread() {}

/// (numéro /dev/videoN, nom) des périphériques de capture vidéo, triés.
fn devices() -> Vec<(usize, String)> {
    let mut nodes: Vec<_> = v4l::context::enum_devices().into_iter().map(|n| n.index()).collect();
    nodes.sort_unstable();
    nodes
        .into_iter()
        .filter_map(|i| {
            let caps = Device::new(i).ok()?.query_caps().ok()?;
            // Une webcam expose aussi des nœuds de métadonnées : on les écarte.
            caps.capabilities
                .contains(Flags::VIDEO_CAPTURE)
                .then_some((i, caps.card))
        })
        .collect()
}

pub fn list_cameras() -> Vec<String> {
    devices().into_iter().map(|(_, n)| n).collect()
}

pub struct Camera {
    stream: Stream<'static>,
    fourcc: FourCC,
    pub w: usize,
    pub h: usize,
    stride: usize,
    pub name: String,
}

impl Camera {
    pub fn open(index: usize, name: &str, want: [u32; 2]) -> Result<Self, String> {
        let devs = devices();
        let (node, cam_name) = devs
            .iter()
            .find(|(_, n)| !name.is_empty() && n == name)
            .or_else(|| devs.get(index))
            .cloned()
            .ok_or_else(|| "aucune webcam trouvée".to_string())?;
        let dev = Device::new(node).map_err(|e| e.to_string())?;
        let mut fmt = dev.format().map_err(|e| e.to_string())?;
        let mut chosen = None;
        for code in [b"YUYV", b"MJPG"] {
            fmt.width = want[0];
            fmt.height = want[1];
            fmt.fourcc = FourCC::new(code);
            if let Ok(f) = dev.set_format(&fmt) {
                if f.fourcc == FourCC::new(code) {
                    chosen = Some(f);
                    break;
                }
            }
        }
        let fmt = chosen.ok_or("format vidéo non pris en charge (YUYV/MJPEG requis)")?;
        let stream = Stream::with_buffers(&dev, Type::VideoCapture, 2).map_err(|e| e.to_string())?;
        Ok(Camera {
            stream,
            fourcc: fmt.fourcc,
            w: fmt.width as usize,
            h: fmt.height as usize,
            stride: fmt.stride.max(fmt.width * 2) as usize,
            name: cam_name,
        })
    }

    /// `fresh` : les deux tampons du pilote peuvent contenir des images
    /// anciennes ; on les écarte pour obtenir une image prise maintenant.
    pub fn read(&mut self, fresh: bool) -> Result<Frame, String> {
        if fresh {
            for _ in 0..2 {
                self.stream.next().map_err(|e| e.to_string())?;
            }
        }
        let (buf, meta) = self.stream.next().map_err(|e| e.to_string())?;
        let data = &buf[..(meta.bytesused as usize).min(buf.len())];
        let (w, h) = (self.w, self.h);
        let mut bgra = vec![0u8; w * h * 4];
        if self.fourcc == FourCC::new(b"MJPG") {
            let mut dec = jpeg_decoder::Decoder::new(data);
            let rgb = dec.decode().map_err(|e| e.to_string())?;
            let info = dec.info().ok_or("image JPEG invalide")?;
            let (iw, ih) = (info.width as usize, info.height as usize);
            let px = match info.pixel_format {
                jpeg_decoder::PixelFormat::RGB24 => 3,
                jpeg_decoder::PixelFormat::L8 => 1,
                _ => return Err("format JPEG non pris en charge".into()),
            };
            let (w, h) = (w.min(iw), h.min(ih));
            for y in 0..h {
                for x in 0..w {
                    let s = (y * iw + x) * px;
                    let o = (y * self.w + x) * 4;
                    let (r, g, b) = if px == 3 {
                        (rgb[s], rgb[s + 1], rgb[s + 2])
                    } else {
                        (rgb[s], rgb[s], rgb[s])
                    };
                    bgra[o..o + 4].copy_from_slice(&[b, g, r, 255]);
                }
            }
        } else {
            super::yuyv_to_bgra(data, w, h, self.stride, &mut bgra);
        }
        Ok(Frame { w, h, bgra })
    }
}
