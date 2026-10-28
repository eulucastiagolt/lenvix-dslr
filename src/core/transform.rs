//! Transformação de frame do pipeline RGB (puro Rust, nearest-neighbor).
//!
//! Após decode e espelho:
//!   crop (aspecto = saída; tamanho controlado pelo zoom; posição = pan ±100%)
//!   → escala para o tamanho de saída escolhido (ex.: 1280×720 p/ OBS 16:9).
//! dimensões sempre pares (YUYV 4:2:2 precisa de pares de pixels).

#[derive(Debug, Clone, Copy)]
pub struct Transform {
    /// 1.0 = sem zoom; 2.0 = metade da área
    pub zoom: f32,
    /// pan X: -100 ..= 100 (0 = centrado; ±100 = borda)
    pub pan_x: i16,
    /// pan Y: -100 ..= 100
    pub pan_y: i16,
    /// tamanho de saída (None = nativo do crop)
    pub out: Option<(u32, u32)>,
}

impl Default for Transform {
    fn default() -> Self {
        Self { zoom: 1.0, pan_x: 0, pan_y: 0, out: None }
    }
}

impl Transform {
    pub fn native(&self) -> bool {
        self.zoom <= 1.0 && self.out.is_none() && self.pan_x == 0 && self.pan_y == 0
    }
}

/// crop+escala. Retorna (w, h, rgb novo). A fonte NÃO é modificada.
#[allow(clippy::too_many_lines)]
pub fn apply(src: &[u8], sw: u32, sh: u32, t: &Transform) -> (u32, u32, Vec<u8>) {
    if t.native() {
        return (sw, sh, src.to_vec());
    }
    let (ow, oh) = t.out.unwrap_or((sw, sh));
    // saída definida? recorta-cobre (crop-fill) no aspecto da SAÍDA, sem
    // distorção: maior retângulo do aspecto alvo dentro do frame, depois
    // escala para (ow, oh). out=None mantém aspecto nativo (zoom/pan só recortam).
    let aspect = ow as f32 / oh as f32;

    // maior retângulo inteiro do aspecto desejado dentro do frame
    let (full_w, full_h) = if sw as f32 / sh as f32 > aspect {
        // fonte mais larga que o alvo: altura cheia, largura cortada
        ((sh as f32 * aspect) as u32, sh)
    } else {
        (sw, (sw as f32 / aspect) as u32)
    };
    let zoom = t.zoom.max(1.0);
    let cw0 = ((full_w as f32 / zoom) as u32 & !1).clamp(2, sw & !1);
    let ch0 = ((full_h as f32 / zoom) as u32 & !1).clamp(2, sh & !1);

    // pan: 0 = centro; ±100 = encostar na borda (livre/2 no máximo)
    let free = |avail: u32, need: u32| -> i32 {
        let f = avail as i32 - need as i32;
        f.max(0)
    };
    let map_pan = |pan: i16, free: i32| -> i32 {
        if free == 0 {
            0
        } else {
            (f64::from(pan) / 100.0 * (free as f64 / 2.0)).round() as i32
        }
    };
    let crop_x = (free(sw, cw0) / 2 + map_pan(t.pan_x, free(sw, cw0)))
        .clamp(0, (sw - cw0) as i32)
        & !1; // par: alinhamento de par de pixels do YUYV
    let crop_y = (free(sh, ch0) / 2 + map_pan(t.pan_y, free(sh, ch0)))
        .clamp(0, (sh - ch0) as i32)
        & !1;

    let (dw, dh) = (ow & !1, oh & !1);

    let dst = vec![0u8; (dw * dh * 3) as usize];
    // sampling nearest com passo em ponto fixo 16.16
    let sx_step = ((cw0 as u64) << 16) / dw as u64;
    let sy_step = ((ch0 as u64) << 16) / dh as u64;
    let mut dst_p = dst;
    for dy in 0..dh as u64 {
        let sy0 = crop_y as u64 + ((dy * sy_step) >> 16);
        let srow = sy0 as usize * sw as usize * 3 + crop_x as usize * 3;
        let drow = dy as usize * dw as usize * 3;
        for dx in 0..dw as u64 {
            let sx = ((dx * sx_step) >> 16) as usize * 3;
            let d = drow + dx as usize * 3;
            dst_p[d] = src[srow + sx];
            dst_p[d + 1] = src[srow + sx + 1];
            dst_p[d + 2] = src[srow + sx + 2];
        }
    }
    (dw, dh, dst_p)
}
