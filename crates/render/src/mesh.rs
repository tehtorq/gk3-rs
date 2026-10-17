//! Turning a room into GPU-ready data, on the CPU.
//!
//! Polygons become a flat triangle list grouped by diffuse texture, so a room
//! draws in one call per texture. Every surface's lightmap is packed into one
//! atlas; each vertex carries where its lightmap sits so the shader can clamp
//! sampling to that rectangle.

use gk3_formats::bsp::Bsp;
use gk3_formats::image::Image;

/// Surfaces with these flags draw without their lightmap (G-Engine's
/// `kIgnoreLightmapFlag` and `kShadowTextureFlag`).
const NO_LIGHTMAP_FLAGS: u32 = 8 | 64;

/// A surface whose texture is a shadow to darken what lies under it, which
/// the original draws in a pass of its own after the room, multiplying it
/// onto the picture with no lightmap (0x5938e0).
const SHADOW_SURFACE_FLAG: u32 = 64;

/// GK3's "2x lighting": lightmaps are doubled so they can brighten as well as
/// darken.
const LIGHTMAP_MULTIPLIER: f32 = 2.0;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    /// This vertex's coordinate within its surface's lightmap, 0..1.
    pub lightmap_uv: [f32; 2],
    /// The lightmap's rectangle in the atlas, in texels: x, y, width, height.
    pub lightmap_rect: [f32; 4],
    /// Lightmap brightness multiplier; zero means the surface is unlit by
    /// lightmaps and draws at full brightness.
    pub lightmap_multiplier: f32,
}

/// A run of vertices sharing a diffuse texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Batch {
    pub texture: usize,
    pub first: u32,
    pub count: u32,
    /// A shadow painted on a surface (flag 64): drawn after the rest, its
    /// texture multiplied onto what lies beneath.
    pub decal: bool,
}

/// The six faces of a skybox in cube-map layer order: +X, -X, +Y, -Y, +Z, -Z.
#[derive(Clone)]
pub struct Sky {
    pub faces: [Image; 6],
    /// Rotation about the vertical axis, in degrees.
    pub azimuth: f32,
}

/// A room ready for upload.
pub struct SceneMesh {
    pub vertices: Vec<Vertex>,
    pub batches: Vec<Batch>,
    pub textures: Vec<Image>,
    pub lightmap_atlas: Image,
    pub sky: Option<Sky>,
    pub bounds: ([f32; 3], [f32; 3]),
}

/// What a room is built from.
pub struct SceneSource<'a> {
    pub bsp: &'a Bsp,
    /// Lightmap *i* lights surface *i*. Missing or short sets leave the
    /// remaining surfaces unlit by lightmaps.
    pub lightmaps: &'a [Image],
    /// Finds a texture by the name a surface gives it.
    pub texture: &'a dyn Fn(&str) -> Option<Image>,
    /// Whether a named BSP object starts hidden.
    pub hidden: &'a dyn Fn(&str) -> bool,
    /// A texture painted over a whole BSP object, by its name, in place of
    /// what its surfaces say.
    pub retexture: &'a dyn Fn(&str) -> Option<String>,
    pub sky: Option<Sky>,
}

impl SceneMesh {
    pub fn build(source: SceneSource) -> Self {
        let bsp = source.bsp;

        // Distinct textures, in first-use order; failures get a placeholder.
        let mut names: Vec<String> = Vec::new();
        let mut texture_of_surface = Vec::with_capacity(bsp.surfaces.len());
        let retextured: Vec<Option<String>> =
            bsp.objects.iter().map(|o| (source.retexture)(o)).collect();
        for surface in &bsp.surfaces {
            let key = retextured
                .get(surface.object as usize)
                .cloned()
                .flatten()
                .unwrap_or_else(|| surface.texture.to_ascii_uppercase());
            let index = match names.iter().position(|n| *n == key) {
                Some(i) => i,
                None => {
                    names.push(key);
                    names.len() - 1
                }
            };
            texture_of_surface.push(index);
        }
        let textures: Vec<Image> = names
            .iter()
            .map(|n| (source.texture)(n).unwrap_or_else(missing_texture))
            .collect();

        let atlas = Atlas::pack(source.lightmaps);
        let hidden: Vec<bool> = bsp.objects.iter().map(|o| (source.hidden)(o)).collect();

        // Polygons sorted by texture, so each texture is one contiguous batch.
        let mut order: Vec<usize> = (0..bsp.polygons.len())
            .filter(|&p| {
                let surface = &bsp.surfaces[bsp.polygons[p].surface as usize];
                !hidden[surface.object as usize]
            })
            .collect();
        let decal = |p: usize| bsp.surfaces[bsp.polygons[p].surface as usize].flags & SHADOW_SURFACE_FLAG != 0;
        order.sort_by_key(|&p| (decal(p), texture_of_surface[bsp.polygons[p].surface as usize]));

        let mut vertices = Vec::new();
        let mut batches: Vec<Batch> = Vec::new();
        for p in order {
            let polygon = &bsp.polygons[p];
            let s = polygon.surface as usize;
            let surface = &bsp.surfaces[s];
            let texture = texture_of_surface[s];
            let (rect, multiplier) = match atlas.rects.get(s) {
                Some(&rect) if surface.flags & NO_LIGHTMAP_FLAGS == 0 => {
                    (rect, LIGHTMAP_MULTIPLIER)
                }
                _ => ([0.0; 4], 0.0),
            };

            let start = vertices.len() as u32;
            for triangle in bsp.triangles(polygon) {
                for index in triangle {
                    let uv = bsp.uv(index);
                    // G-Engine's shader: (uv + offset) * scale.
                    let lightmap_uv = [
                        (uv[0] + surface.lightmap_offset[0]) * surface.lightmap_scale[0],
                        (uv[1] + surface.lightmap_offset[1]) * surface.lightmap_scale[1],
                    ];
                    vertices.push(Vertex {
                        position: bsp.vertices[index as usize],
                        uv,
                        lightmap_uv,
                        lightmap_rect: rect,
                        lightmap_multiplier: multiplier,
                    });
                }
            }
            let added = vertices.len() as u32 - start;
            let is_decal = decal(p);
            match batches.last_mut() {
                Some(b) if b.texture == texture && b.decal == is_decal => b.count += added,
                _ => batches.push(Batch {
                    texture,
                    first: start,
                    count: added,
                    decal: is_decal,
                }),
            }
        }

        Self {
            vertices,
            batches,
            textures,
            lightmap_atlas: atlas.image,
            sky: source.sky,
            bounds: bounds(&bsp.vertices),
        }
    }
}

impl Sky {
    /// Builds a skybox from the six faces as GK3 names them, applying the
    /// corrections G-Engine found: the "front" texture faces -Z and "back"
    /// faces +Z, and the up and down textures are rotated a quarter turn.
    /// A missing down face is black, as below the horizon nothing shows
    /// through; other missing faces repeat one that exists.
    pub fn from_named(
        left: Option<Image>,
        right: Option<Image>,
        front: Option<Image>,
        back: Option<Image>,
        up: Option<Image>,
        down: Option<Image>,
        azimuth: f32,
    ) -> Option<Self> {
        let up = up.map(|i| rotate_counterclockwise(&i));
        let down = down.map(|i| rotate_counterclockwise(&i));
        let mut faces = [right, left, up, down, back, front];
        let fallback = faces.iter().flatten().next()?.clone();
        let size = faces
            .iter()
            .flatten()
            .map(|f| f.width.max(f.height))
            .max()?;
        if faces[3].is_none() {
            faces[3] = Some(Image {
                width: 1,
                height: 1,
                rgba: vec![0, 0, 0, 255],
            });
        }
        let faces = faces.map(|f| resize_square(&f.unwrap_or_else(|| fallback.clone()), size));
        Some(Self { faces, azimuth })
    }
}

/// A grey and pink checker that makes a missing texture obvious.
fn missing_texture() -> Image {
    let mut rgba = Vec::with_capacity(8 * 8 * 4);
    for y in 0..8 {
        for x in 0..8 {
            let c = if (x / 4 + y / 4) % 2 == 0 {
                [128, 128, 128, 255]
            } else {
                [200, 80, 160, 255]
            };
            rgba.extend_from_slice(&c);
        }
    }
    Image {
        width: 8,
        height: 8,
        rgba,
    }
}

fn bounds(vertices: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for v in vertices {
        for a in 0..3 {
            min[a] = min[a].min(v[a]);
            max[a] = max[a].max(v[a]);
        }
    }
    if vertices.is_empty() {
        ([0.0; 3], [0.0; 3])
    } else {
        (min, max)
    }
}

/// Rotates an image a quarter turn counter-clockwise: the top-right corner
/// becomes the top-left.
pub fn rotate_counterclockwise(image: &Image) -> Image {
    let (w, h) = (image.width, image.height);
    let mut rgba = vec![0u8; image.rgba.len()];
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = (y, w - 1 - x);
            let src = ((y * w + x) * 4) as usize;
            let dst = ((ny * h + nx) * 4) as usize;
            rgba[dst..dst + 4].copy_from_slice(&image.rgba[src..src + 4]);
        }
    }
    Image {
        width: h,
        height: w,
        rgba,
    }
}

/// Nearest-neighbour resize to a square; cube faces must all match.
fn resize_square(image: &Image, size: u32) -> Image {
    if image.width == size && image.height == size {
        return image.clone();
    }
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let sx = x * image.width / size;
            let sy = y * image.height / size;
            rgba.extend_from_slice(&image.pixel(sx, sy));
        }
    }
    Image {
        width: size,
        height: size,
        rgba,
    }
}

/// Halves an image for the next mipmap level: Lanczos-2, in linear light,
/// so the smaller levels keep the detail a box filter blurs away and fine
/// patterns keep their brightness. Alpha (the colour key's cut-outs) is
/// box-filtered, as the sharper filter's overshoot would speckle its edges.
pub fn half(image: &Image) -> Image {
    let (w, h) = ((image.width / 2).max(1), (image.height / 2).max(1));
    let (sw, sh) = (image.width as usize, image.height as usize);
    // Colour in linear light, alpha as it is.
    let linear: Vec<[f32; 3]> = image
        .rgba
        .chunks_exact(4)
        .map(|p| [0, 1, 2].map(|c| srgb_to_linear(p[c])))
        .collect();
    let across = resample_rows(&linear, sw, sh, w as usize);
    let colour = resample_columns(&across, w as usize, sh, h as usize);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let c = colour[(y * w + x) as usize];
            rgba.extend(c.map(linear_to_srgb));
            let mut alpha = 0u32;
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let sx = (x * 2 + dx).min(image.width - 1);
                let sy = (y * 2 + dy).min(image.height - 1);
                alpha += image.pixel(sx, sy)[3] as u32;
            }
            rgba.push(((alpha + 2) / 4) as u8);
        }
    }
    Image {
        width: w,
        height: h,
        rgba,
    }
}

fn srgb_to_linear(v: u8) -> f32 {
    (v as f32 / 255.0).powf(2.2)
}

fn linear_to_srgb(v: f32) -> u8 {
    (v.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0).round() as u8
}

/// The Lanczos-2 kernel.
fn lanczos2(x: f32) -> f32 {
    let x = x.abs();
    if x < 1e-6 {
        1.0
    } else if x >= 2.0 {
        0.0
    } else {
        let px = std::f32::consts::PI * x;
        2.0 * px.sin() * (px / 2.0).sin() / (px * px)
    }
}

/// For each of `to` new samples across `from` old ones: which old samples
/// it takes, and their weights (summing to one). Edges are clamped.
fn taps(from: usize, to: usize) -> Vec<Vec<(usize, f32)>> {
    let scale = from as f32 / to as f32;
    let radius = 2.0 * scale;
    (0..to)
        .map(|i| {
            let centre = (i as f32 + 0.5) * scale;
            let first = (centre - radius).floor() as isize;
            let last = (centre + radius).ceil() as isize;
            let mut t: Vec<(usize, f32)> = (first..=last)
                .map(|j| {
                    let d = (j as f32 + 0.5 - centre) / scale;
                    (j.clamp(0, from as isize - 1) as usize, lanczos2(d))
                })
                .filter(|(_, wt)| *wt != 0.0)
                .collect();
            let sum: f32 = t.iter().map(|(_, wt)| wt).sum();
            for (_, wt) in &mut t {
                *wt /= sum;
            }
            t
        })
        .collect()
}

fn resample_rows(src: &[[f32; 3]], w: usize, h: usize, to: usize) -> Vec<[f32; 3]> {
    let taps = taps(w, to);
    let mut out = Vec::with_capacity(to * h);
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        for t in &taps {
            let mut c = [0.0f32; 3];
            for &(j, wt) in t {
                for k in 0..3 {
                    c[k] += row[j][k] * wt;
                }
            }
            out.push(c);
        }
    }
    out
}

fn resample_columns(src: &[[f32; 3]], w: usize, h: usize, to: usize) -> Vec<[f32; 3]> {
    let taps = taps(h, to);
    let mut out = vec![[0.0f32; 3]; w * to];
    for (i, t) in taps.iter().enumerate() {
        for x in 0..w {
            let mut c = [0.0f32; 3];
            for &(j, wt) in t {
                for k in 0..3 {
                    c[k] += src[j * w + x][k] * wt;
                }
            }
            out[i * w + x] = c;
        }
    }
    out
}

/// Lightmaps packed into one image with a shelf packer. Each lightmap gets a
/// one-texel border copied from its edge, so filtering at the edge of a
/// rectangle never picks up a neighbour.
struct Atlas {
    image: Image,
    /// Per lightmap: x, y, width, height of its interior, in texels.
    rects: Vec<[f32; 4]>,
}

impl Atlas {
    fn pack(maps: &[Image]) -> Self {
        if maps.is_empty() {
            return Self {
                image: Image {
                    width: 1,
                    height: 1,
                    rgba: vec![255; 4],
                },
                rects: Vec::new(),
            };
        }
        let padded = |m: &Image| (m.width + 2, m.height + 2);
        let area: u64 = maps
            .iter()
            .map(|m| {
                let (w, h) = padded(m);
                w as u64 * h as u64
            })
            .sum();
        let widest = maps.iter().map(|m| m.width + 2).max().unwrap_or(1);
        let width = ((area as f64 * 1.15).sqrt().ceil() as u32)
            .next_power_of_two()
            .max(widest.next_power_of_two())
            .max(64);

        // Tallest first packs shelves tightly.
        let mut order: Vec<usize> = (0..maps.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(maps[i].height));
        let mut placed = vec![(0u32, 0u32); maps.len()];
        let (mut x, mut y, mut shelf) = (0u32, 0u32, 0u32);
        for &i in &order {
            let (w, h) = padded(&maps[i]);
            if x + w > width {
                x = 0;
                y += shelf;
                shelf = 0;
            }
            placed[i] = (x, y);
            x += w;
            shelf = shelf.max(h);
        }
        let height = (y + shelf).max(1);

        let mut image = Image {
            width,
            height,
            rgba: vec![255; (width * height * 4) as usize],
        };
        let mut rects = Vec::with_capacity(maps.len());
        for (map, &(px, py)) in maps.iter().zip(&placed) {
            for dy in 0..map.height + 2 {
                for dx in 0..map.width + 2 {
                    let sx = dx.saturating_sub(1).min(map.width - 1);
                    let sy = dy.saturating_sub(1).min(map.height - 1);
                    let dst = (((py + dy) * width + px + dx) * 4) as usize;
                    image.rgba[dst..dst + 4].copy_from_slice(&map.pixel(sx, sy));
                }
            }
            rects.push([
                (px + 1) as f32,
                (py + 1) as f32,
                map.width as f32,
                map.height as f32,
            ]);
        }
        Self { image, rects }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, c: [u8; 4]) -> Image {
        Image {
            width: w,
            height: h,
            rgba: c
                .iter()
                .copied()
                .cycle()
                .take((w * h * 4) as usize)
                .collect(),
        }
    }

    #[test]
    fn atlas_places_every_map_without_overlap() {
        let maps: Vec<Image> = (0..40)
            .map(|i| solid(1 + i % 7, 1 + i % 5, [i as u8, 0, 0, 255]))
            .collect();
        let atlas = Atlas::pack(&maps);
        for (i, (map, r)) in maps.iter().zip(&atlas.rects).enumerate() {
            let (x, y) = (r[0] as u32, r[1] as u32);
            assert_eq!((r[2] as u32, r[3] as u32), (map.width, map.height));
            // Interior and border both hold this map's colour.
            assert_eq!(atlas.image.pixel(x, y)[0], i as u8);
            assert_eq!(atlas.image.pixel(x - 1, y - 1)[0], i as u8);
            assert_eq!(atlas.image.pixel(x + map.width, y + map.height)[0], i as u8);
        }
    }

    #[test]
    fn rotation_moves_top_right_to_top_left() {
        let mut image = solid(2, 3, [0, 0, 0, 255]);
        image.rgba[4..8].copy_from_slice(&[9, 9, 9, 255]); // (1, 0): top right
        let rotated = rotate_counterclockwise(&image);
        assert_eq!((rotated.width, rotated.height), (3, 2));
        assert_eq!(rotated.pixel(0, 0), [9, 9, 9, 255]);
    }

    #[test]
    fn halving_keeps_flat_colour_and_brightness() {
        // A flat colour stays that colour.
        let flat = solid(8, 8, [120, 60, 200, 255]);
        let small = half(&flat);
        assert_eq!((small.width, small.height), (4, 4));
        for y in 0..4 {
            for x in 0..4 {
                let p = small.pixel(x, y);
                assert!(p.iter().zip([120, 60, 200, 255]).all(|(a, b)| a.abs_diff(b) <= 1), "{p:?}");
            }
        }
        // A fine black and white check goes to an even grey of the same
        // brightness (half the light: 186, not 128).
        let mut check = solid(8, 8, [0, 0, 0, 255]);
        for y in 0..8u32 {
            for x in 0..8u32 {
                if (x + y) % 2 == 0 {
                    let i = ((y * 8 + x) * 4) as usize;
                    check.rgba[i..i + 3].copy_from_slice(&[255, 255, 255]);
                }
            }
        }
        let grey = half(&check);
        for y in 0..4 {
            for x in 0..4 {
                let p = grey.pixel(x, y);
                assert!(p[0].abs_diff(186) <= 6 && p[0] == p[1] && p[1] == p[2], "{p:?}");
            }
        }
        // Alpha is averaged, as a box filter would.
        let mut cut = solid(2, 2, [10, 20, 30, 255]);
        cut.rgba[3] = 0;
        assert_eq!(half(&cut).pixel(0, 0)[3], 191);
    }
}
