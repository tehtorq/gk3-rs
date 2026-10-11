//! The interface layer: rectangles, images and text drawn over the scene in
//! screen pixels. Every element is a textured quad tinted by a colour; a
//! plain rectangle uses a white texture.

/// A texture the overlay can draw from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OverlayTexture(pub(crate) usize);

/// The white texture every renderer starts with, for solid rectangles.
pub const WHITE: OverlayTexture = OverlayTexture(0);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad {
    pub texture: OverlayTexture,
    /// Source rectangle in texture pixels: x, y, width, height.
    pub source: [f32; 4],
    /// Destination in screen pixels from the top left: x, y, width, height.
    pub dest: [f32; 4],
    pub color: [u8; 4],
}

/// One frame's interface, drawn in order.
#[derive(Debug, Default, Clone)]
pub struct Overlay {
    pub quads: Vec<Quad>,
}

impl Overlay {
    pub fn clear(&mut self) {
        self.quads.clear();
    }

    pub fn rect(&mut self, dest: [f32; 4], color: [u8; 4]) {
        self.quads.push(Quad {
            texture: WHITE,
            source: [0.0, 0.0, 1.0, 1.0],
            dest,
            color,
        });
    }

    pub fn image(
        &mut self,
        texture: OverlayTexture,
        source: [f32; 4],
        dest: [f32; 4],
        color: [u8; 4],
    ) {
        self.quads.push(Quad {
            texture,
            source,
            dest,
            color,
        });
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct OverlayVertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
    pub color: [u8; 4],
}

/// Six vertices per quad, in clip space.
pub(crate) fn vertices(
    overlay: &Overlay,
    sizes: &[(u32, u32)],
    screen: (u32, u32),
) -> Vec<OverlayVertex> {
    let (sw, sh) = (screen.0.max(1) as f32, screen.1.max(1) as f32);
    let mut out = Vec::with_capacity(overlay.quads.len() * 6);
    for q in &overlay.quads {
        let (tw, th) = sizes
            .get(q.texture.0)
            .map_or((1.0, 1.0), |&(w, h)| (w as f32, h as f32));
        let [x, y, w, h] = q.dest;
        let (x0, x1) = (x / sw * 2.0 - 1.0, (x + w) / sw * 2.0 - 1.0);
        let (y0, y1) = (1.0 - y / sh * 2.0, 1.0 - (y + h) / sh * 2.0);
        let [sx, sy, swid, shei] = q.source;
        let (u0, u1) = (sx / tw, (sx + swid) / tw);
        let (v0, v1) = (sy / th, (sy + shei) / th);
        let v = |position, uv| OverlayVertex {
            position,
            uv,
            color: q.color,
        };
        out.extend([
            v([x0, y0], [u0, v0]),
            v([x1, y0], [u1, v0]),
            v([x0, y1], [u0, v1]),
            v([x0, y1], [u0, v1]),
            v([x1, y0], [u1, v0]),
            v([x1, y1], [u1, v1]),
        ]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quads_map_pixels_to_clip_space() {
        let mut o = Overlay::default();
        o.rect([0.0, 0.0, 50.0, 25.0], [255; 4]);
        let v = vertices(&o, &[(1, 1)], (100, 50));
        assert_eq!(v[0].position, [-1.0, 1.0]);
        assert_eq!(v[5].position, [0.0, 0.0]);
        assert_eq!(v[5].uv, [1.0, 1.0]);
    }
}
