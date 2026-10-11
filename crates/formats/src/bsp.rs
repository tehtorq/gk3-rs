//! Room geometry (`.BSP`).
//!
//! Layout follows G-Engine's `BSP::ParseFromData`. `SCEN` is written `NECS`.
//! The format has no trailer, so integrity is checked through its
//! cross-references: polygon index runs, vertex indices, surface and object
//! indices must all land inside their arrays.

use crate::reader::Reader;
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct Bsp {
    /// Format version as (major, minor), stored minor first. 103 retail
    /// scenes are 2.2; one is 1.2, five are 1.1 and DEFAULT.BSP is 1.0.
    pub version: (u8, u8),
    /// Named objects ("door", "bed"...); surfaces belong to one each.
    pub objects: Vec<String>,
    pub surfaces: Vec<Surface>,
    pub polygons: Vec<Polygon>,
    pub vertices: Vec<[f32; 3]>,
    /// Texture coordinates. Can be fewer than vertices (`DEFAULT.BSP`); see
    /// [`Bsp::uv`].
    pub uvs: Vec<[f32; 2]>,
    /// Vertex indices that polygons refer to in runs.
    pub indices: Vec<u16>,
    pub nodes: Vec<Node>,
    pub planes: Vec<[f32; 4]>,
    pub root: u32,
    /// Light emitters baked into surfaces, where the file has them.
    pub lights: Vec<SurfaceLight>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Surface {
    pub object: u32,
    pub texture: String,
    pub lightmap_offset: [f32; 2],
    pub lightmap_scale: [f32; 2],
    pub flags: u32,
}

/// A convex polygon, fan-triangulated from its first vertex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Polygon {
    /// Start of this polygon's run in [`Bsp::indices`].
    pub first_index: u16,
    pub index_count: u16,
    pub surface: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Node {
    pub front: u16,
    pub back: u16,
    pub plane: u16,
    pub polygon: u16,
    pub polygon2: u16,
    pub polygon_count: u16,
    pub polygon_count2: u16,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceLight {
    pub surface: u32,
    pub position: [f32; 3],
    pub radius: f32,
}

impl Bsp {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader::new(bytes);
        r.tag(b"NECS")?;
        let minor = r.u8()?;
        let major = r.u8()?;
        r.skip(6)?; // two unknown bytes, content size
        let root = r.u32()?;
        let counts: Vec<usize> = (0..9)
            .map(|_| r.u32().map(|n| n as usize))
            .collect::<Result<_>>()?;
        let [
            name_count,
            vertex_count,
            uv_count,
            index_count,
            other_index_count,
            surface_count,
            plane_count,
            node_count,
            polygon_count,
        ] = counts[..]
        else {
            unreachable!()
        };

        r.expect_room(name_count, 32)?;
        let objects = (0..name_count)
            .map(|_| r.fixed_str(32))
            .collect::<Result<Vec<_>>>()?;

        r.expect_room(surface_count, 56)?;
        let mut surfaces = Vec::with_capacity(surface_count);
        for _ in 0..surface_count {
            let object = r.u32()?;
            let texture = r.fixed_str(32)?;
            let lightmap_offset = r.vec2()?;
            let lightmap_scale = r.vec2()?;
            r.skip(4)?; // unknown float
            let flags = r.u32()?;
            surfaces.push(Surface {
                object,
                texture,
                lightmap_offset,
                lightmap_scale,
                flags,
            });
        }

        r.expect_room(node_count, 16)?;
        let mut nodes = Vec::with_capacity(node_count);
        for _ in 0..node_count {
            nodes.push(Node {
                front: r.u16()?,
                back: r.u16()?,
                plane: r.u16()?,
                polygon: r.u16()?,
                polygon2: r.u16()?,
                polygon_count: r.u16()?,
                polygon_count2: r.u16()?,
            });
            r.skip(2)?; // unknown, constant within a file
        }

        r.expect_room(polygon_count, 8)?;
        let mut polygons = Vec::with_capacity(polygon_count);
        for _ in 0..polygon_count {
            let first_index = r.u16()?;
            r.skip(2)?; // unknown, usually 1073
            let index_count = r.u16()?;
            let surface = r.u16()?;
            polygons.push(Polygon {
                first_index,
                index_count,
                surface,
            });
        }

        r.expect_room(plane_count, 16)?;
        let planes = (0..plane_count)
            .map(|_| Ok([r.f32()?, r.f32()?, r.f32()?, r.f32()?]))
            .collect::<Result<_>>()?;
        r.expect_room(vertex_count, 12)?;
        let vertices = (0..vertex_count).map(|_| r.vec3()).collect::<Result<_>>()?;
        r.expect_room(uv_count, 8)?;
        let uvs = (0..uv_count).map(|_| r.vec2()).collect::<Result<_>>()?;
        r.expect_room(index_count, 2)?;
        let indices = (0..index_count).map(|_| r.u16()).collect::<Result<_>>()?;

        // A second index array, identical to the first in every retail file.
        r.skip(other_index_count * 2)?;
        // A bounding sphere per node.
        r.skip(node_count * 16)?;

        // Per-surface light data. Absent from DEFAULT.BSP (1.0), and laid out
        // differently in the five 1.1 scenes, whose trailing data is skipped.
        let mut lights = Vec::new();
        if (major, minor) >= (1, 2) && r.remaining() > 0 {
            for surface in 0..surface_count as u32 {
                let position = r.vec3()?;
                let radius = r.f32()?;
                r.skip(12)?;
                let index_count = r.u32()? as usize;
                let triangle_count = r.u32()? as usize;
                r.skip(index_count * 2)?;
                r.skip(triangle_count * 6)?;
                if radius > 0.0 {
                    lights.push(SurfaceLight {
                        surface,
                        position,
                        radius,
                    });
                }
            }
        }
        if r.remaining() != 0 && (major, minor) >= (1, 2) {
            return Err(Error::Format(format!(
                "{} bytes left after the light section",
                r.remaining()
            )));
        }

        let bsp = Self {
            version: (major, minor),
            objects,
            surfaces,
            polygons,
            vertices,
            uvs,
            indices,
            nodes,
            planes,
            root,
            lights,
        };
        bsp.validate()?;
        Ok(bsp)
    }

    fn validate(&self) -> Result<()> {
        let fail = |message: String| Err(Error::Format(message));
        for (i, s) in self.surfaces.iter().enumerate() {
            if s.object as usize >= self.objects.len() {
                return fail(format!(
                    "surface {i} names object {} of {}",
                    s.object,
                    self.objects.len()
                ));
            }
        }
        for (i, p) in self.polygons.iter().enumerate() {
            let end = p.first_index as usize + p.index_count as usize;
            if end > self.indices.len() {
                return fail(format!(
                    "polygon {i} runs to index {end} of {}",
                    self.indices.len()
                ));
            }
            if p.surface as usize >= self.surfaces.len() {
                return fail(format!(
                    "polygon {i} names surface {} of {}",
                    p.surface,
                    self.surfaces.len()
                ));
            }
            if p.index_count < 3 {
                return fail(format!("polygon {i} has {} vertices", p.index_count));
            }
        }
        if let Some(bad) = self
            .indices
            .iter()
            .find(|&&i| i as usize >= self.vertices.len())
        {
            return fail(format!("index {bad} past {} vertices", self.vertices.len()));
        }
        Ok(())
    }

    /// A vertex's texture coordinate, or the origin where the file has fewer
    /// coordinates than vertices.
    pub fn uv(&self, vertex: u16) -> [f32; 2] {
        self.uvs.get(vertex as usize).copied().unwrap_or([0.0, 0.0])
    }

    /// Vertex indices of a polygon's triangles, fanned from its first vertex.
    pub fn triangles(&self, polygon: &Polygon) -> impl Iterator<Item = [u16; 3]> + '_ {
        let run = &self.indices[polygon.first_index as usize..][..polygon.index_count as usize];
        (1..run.len() - 1).map(move |i| [run[0], run[i], run[i + 1]])
    }

    pub fn triangle_count(&self) -> usize {
        self.polygons
            .iter()
            .map(|p| p.index_count as usize - 2)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One object, one surface, one quad polygon.
    fn quad_scene(with_lights: bool) -> Vec<u8> {
        let u32s =
            |out: &mut Vec<u8>, v: &[u32]| v.iter().for_each(|n| out.extend(n.to_le_bytes()));
        let u16s =
            |out: &mut Vec<u8>, v: &[u16]| v.iter().for_each(|n| out.extend(n.to_le_bytes()));
        let f32s =
            |out: &mut Vec<u8>, v: &[f32]| v.iter().for_each(|n| out.extend(n.to_le_bytes()));

        let mut out = b"NECS".to_vec();
        u32s(&mut out, &[0x0202, 0, 0]);
        // Header: tag, version 2.2, size, root. Counts follow.
        // names, vertices, uvs, indices, other indices, surfaces, planes, nodes, polygons
        u32s(&mut out, &[1, 4, 4, 4, 4, 1, 1, 1, 1]);
        let mut name = [0u8; 32];
        name[..4].copy_from_slice(b"door");
        out.extend(name);
        u32s(&mut out, &[0]);
        let mut texture = [0u8; 32];
        texture[..4].copy_from_slice(b"WOOD");
        out.extend(texture);
        f32s(&mut out, &[0.0, 0.0, 1.0, 1.0, 1.0]);
        u32s(&mut out, &[0]);
        u16s(&mut out, &[0, 0, 0, 0, 0, 1, 0, 0]); // node
        u16s(&mut out, &[0, 1073, 4, 0]); // polygon
        f32s(&mut out, &[0.0, 1.0, 0.0, 0.0]); // plane
        f32s(&mut out, &[0., 0., 0., 1., 0., 0., 1., 0., 1., 0., 0., 1.]);
        f32s(&mut out, &[0., 0., 1., 0., 1., 1., 0., 1.]);
        u16s(&mut out, &[0, 1, 2, 3]);
        u16s(&mut out, &[0, 1, 2, 3]);
        f32s(&mut out, &[0.0; 4]); // node sphere
        if with_lights {
            f32s(&mut out, &[1.0, 2.0, 3.0, 50.0, 0.0, 0.0, 0.0]);
            u32s(&mut out, &[1, 1]);
            u16s(&mut out, &[0, 0, 1, 2]);
        }
        out
    }

    #[test]
    fn parses_and_fans_polygons() {
        let bsp = Bsp::parse(&quad_scene(false)).unwrap();
        assert_eq!(bsp.objects, ["door"]);
        assert_eq!(bsp.surfaces[0].texture, "WOOD");
        let triangles: Vec<_> = bsp.triangles(&bsp.polygons[0]).collect();
        assert_eq!(triangles, [[0, 1, 2], [0, 2, 3]]);
        assert_eq!(bsp.triangle_count(), 2);
        assert!(bsp.lights.is_empty());
    }

    #[test]
    fn reads_surface_lights() {
        let bsp = Bsp::parse(&quad_scene(true)).unwrap();
        assert_eq!(bsp.lights.len(), 1);
        assert_eq!(bsp.lights[0].position, [1.0, 2.0, 3.0]);
    }

    #[test]
    fn early_versions_skip_unknown_trailing_data() {
        let mut bytes = quad_scene(false);
        bytes[4] = 1; // version 1.1: minor first, then major
        bytes[5] = 1;
        bytes.extend([0xFF; 40]);
        let bsp = Bsp::parse(&bytes).unwrap();
        assert_eq!(bsp.version, (1, 1));
        assert!(bsp.lights.is_empty());
    }

    #[test]
    fn bad_cross_references_are_errors() {
        let mut bytes = quad_scene(false);
        // The polygon's index count is the third u16 of the polygon record.
        let polygon_at = 4 + 12 + 36 + 32 + 56 + 16;
        bytes[polygon_at + 4] = 9;
        assert!(Bsp::parse(&bytes).is_err());
    }
}
