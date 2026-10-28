//! Conversão de cores puro Rust (sem libav no MVP).

use crate::core::transform::{self, Transform};
use std::sync::OnceLock;

const STANDBY_JPEG: &[u8] = include_bytes!("../../assets/standby.jpg");

static BASE_STANDBY_RGB: OnceLock<(u32, u32, Vec<u8>)> = OnceLock::new();

fn get_base_standby() -> &'static (u32, u32, Vec<u8>) {
    BASE_STANDBY_RGB.get_or_init(|| {
        let mut decoder = zune_jpeg::JpegDecoder::new(std::io::Cursor::new(STANDBY_JPEG));
        if decoder.decode_headers().is_ok()
            && let Some(info) = decoder.info()
            && let Ok(rgb) = decoder.decode()
        {
            return (info.width as u32, info.height as u32, rgb);
        }
        // Fallback procedural se falhar: 1920x1080 cinza escuro (#151515)
        let (w, h) = (1920u32, 1080u32);
        let rgb = vec![21u8; (w * h * 3) as usize];
        (w, h, rgb)
    })
}

/// RGB8 (interleaved) → YUYV 4:2:2 packed (BT.601, full→limited).
/// YUYV: pares de pixels compartilham U/V: [Y0 U Y1 V] = 4 bytes / 2 pixels.
/// Croma é decimado pela média dos dois pixels do par (4:2:2 fiel).
pub fn rgb_to_yuyv(rgb: &[u8], w: u32, h: u32) -> Vec<u8> {
    debug_assert_eq!(rgb.len(), (w * h * 3) as usize);
    let mut yuyv = vec![0u8; (w * h * 2) as usize];
    for y in 0..h {
        let row = (y * w * 3) as usize;
        let out_row = (y * w * 2) as usize;
        for x_pair in 0..(w / 2) as usize {
            let px = row + x_pair * 6;
            let out = out_row + x_pair * 4;
            let (r0, g0, b0) = (rgb[px] as i32, rgb[px + 1] as i32, rgb[px + 2] as i32);
            let (r1, g1, b1) = (
                rgb.get(px + 3).copied().unwrap_or(r0 as u8) as i32,
                rgb.get(px + 4).copied().unwrap_or(g0 as u8) as i32,
                rgb.get(px + 5).copied().unwrap_or(b0 as u8) as i32,
            );
            // Y por pixel
            yuyv[out] = clamp8(((66 * r0 + 129 * g0 + 25 * b0 + 138) >> 8) + 16);
            yuyv[out + 2] = clamp8(((66 * r1 + 129 * g1 + 25 * b1 + 138) >> 8) + 16);
            // Croma: média dos dois pixels do par (4:2:2; linear ⇒ média exata).
            // escala /256 (>>8): aritmética BT.601 padrão
            let (r, g, b) = ((r0 + r1) / 2, (g0 + g1) / 2, (b0 + b1) / 2);
            let u = ((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128;
            let v = ((112 * r - 94 * g - 18 * b + 128) >> 8) + 128;
            yuyv[out + 1] = clamp8(u);
            yuyv[out + 3] = clamp8(v);
        }
    }
    yuyv
}

#[inline(always)]
fn clamp8(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

/// Espelha RGB horizontalmente (in-place) — para "imagem espelhada" do preview/webcam
pub fn flip_rgb_h(rgb: &mut [u8], w: u32, h: u32) {
    let stride = (w * 3) as usize;
    let half = (w as usize) / 2;
    for y in 0..h as usize {
        let row = &mut rgb[y * stride..(y + 1) * stride];
        for x in 0..half {
            let a = x * 3;
            let b = (w as usize - 1 - x) * 3;
            row.swap(a, b);
            row.swap(a + 1, b + 1);
            row.swap(a + 2, b + 2);
        }
    }
}

/// Frame "standby" (webcam sem câmera conectada): tela fixa estilo Canon EOS Webcam Utility,
/// proporcional e escalada para as dimensões solicitadas (w, h).
pub fn pattern_rgb(w: u32, h: u32) -> Vec<u8> {
    let (bw, bh, base_rgb) = get_base_standby();
    let (target_w, target_h) = (w & !1, h & !1);
    if (*bw, *bh) == (target_w, target_h) {
        return base_rgb.clone();
    }
    let t = Transform {
        zoom: 1.0,
        pan_x: 0,
        pan_y: 0,
        out: Some((target_w, target_h)),
    };
    let (_, _, scaled) = transform::apply(base_rgb, *bw, *bh, &t);
    scaled
}

/// RGB8 → RGBA8 (para textura GTK; opaque alpha 255)
pub fn rgb_to_rgba(rgb: &[u8], out: &mut Vec<u8>) {
    out.clear();
    out.reserve(rgb.len() / 3 * 4);
    for chunk in rgb.as_chunks::<3>().0 {
        out.push(chunk[0]);
        out.push(chunk[1]);
        out.push(chunk[2]);
        out.push(255);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standby_pattern_dimensions() {
        for (w, h) in [(1920, 1080), (1280, 720), (1056, 704), (960, 540)] {
            let rgb = pattern_rgb(w, h);
            assert_eq!(rgb.len(), (w * h * 3) as usize);

            let yuyv = rgb_to_yuyv(&rgb, w, h);
            assert_eq!(yuyv.len(), (w * h * 2) as usize);

            let mut rgba = Vec::new();
            rgb_to_rgba(&rgb, &mut rgba);
            assert_eq!(rgba.len(), (w * h * 4) as usize);
        }
    }
}
