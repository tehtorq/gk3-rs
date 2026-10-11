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
    pub sky: Option<Sky>,
}

impl SceneMesh {
    pub fn build(source: SceneSource) -> Self {
        let bsp = source.bsp;

        // Distinct textures, in first-use order; failures get a placeholder.
        let mut names: Vec<String> = Vec::new();
        let mut texture_of_surface = Vec::with_capacity(bsp.surfaces.len());
        for surface in &bsp.surfaces {
            let key = surface.texture.to_ascii_uppercase();
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
        order.sort_by_key(|&p| texture_of_surface[bsp.polygons[p].surface as usize]);

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
            match batches.last_mut() {
                Some(b) if b.texture == texture => b.count += added,
                _ => batches.push(Batch {
                    texture,
                    first: start,
                    count: added,
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

/// Halves an image with a box filter, for mipmaps.
pub fn half(image: &Image) -> Image {
    let w = (image.width / 2).max(1);
    let h = (image.height / 2).max(1);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let sx = (x * 2 + dx).min(image.width - 1);
                let sy = (y * 2 + dy).min(image.height - 1);
                let p = image.pixel(sx, sy);
                for c in 0..4 {
                    sum[c] += p[c] as u32;
                }
            }
            rgba.extend(sum.map(|s| ((s + 2) / 4) as u8));
        }
    }
    Image {
        width: w,
        height: h,
        rgba,
    }
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
    fn halving_averages() {
        let mut image = solid(2, 2, [0, 0, 0, 255]);
        image.rgba[0] = 200;
        assert_eq!(half(&image).pixel(0, 0), [50, 0, 0, 255]);
    }
}
