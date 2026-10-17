//! Models (`.MOD`): meshes made of textured submeshes.
//!
//! Layout follows G-Engine's `Model::ParseFromData`. Tags are little-endian, so
//! `MODL` is written `LDOM`. The trailing `XDOM` tag is the integrity check: a
//! misread count anywhere moves it.

use crate::Result;
use crate::reader::Reader;

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    /// Drawn facing the camera, like a sprite.
    pub billboard: bool,
    pub meshes: Vec<Mesh>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    /// Mesh-to-model transform as columns: i, j, k basis vectors and position.
    /// The original Z-up to Y-up rotation is already in the basis.
    pub transform: [[f32; 3]; 4],
    pub bounds_min: [f32; 3],
    pub bounds_max: [f32; 3],
    pub submeshes: Vec<Submesh>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Submesh {
    pub texture: String,
    /// Tint as RGB. The file's alpha byte is always zero and is ignored.
    pub color: [u8; 3],
    /// How many vertices at the end are points of their own rather than
    /// part of the shape (left out of its bounds): 4 or 0.
    pub trailing_points: usize,
    pub positions: Vec<[f32; 3]>,
    /// As stored: in model space rather than mesh space.
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub triangles: Vec<[u16; 3]>,
}

impl Model {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader::new(bytes);
        r.tag(b"LDOM")?;
        r.skip(4)?; // minor, major version, then two zero bytes
        let mesh_count = r.u32()? as usize;
        r.skip(4)?; // data size, excluding this 48-byte header
        r.skip(8)?; // unknown
        let flags = r.u32()?;
        r.skip(16)?; // unknown, likely more flags
        r.skip(4)?; // unknown, always 8

        // A mesh with no submeshes is 80 bytes; rejects absurd counts before allocating.
        r.expect_room(mesh_count, 80)?;
        let mut meshes = Vec::with_capacity(mesh_count);
        for _ in 0..mesh_count {
            meshes.push(read_mesh(&mut r)?);
        }
        r.tag(b"XDOM")?;

        Ok(Self {
            billboard: flags & 2 != 0,
            meshes,
        })
    }

    pub fn triangle_count(&self) -> usize {
        self.submeshes().map(|s| s.triangles.len()).sum()
    }

    pub fn vertex_count(&self) -> usize {
        self.submeshes().map(|s| s.positions.len()).sum()
    }

    pub fn submeshes(&self) -> impl Iterator<Item = &Submesh> {
        self.meshes.iter().flat_map(|m| &m.submeshes)
    }
}

fn read_mesh(r: &mut Reader) -> Result<Mesh> {
    r.tag(b"HSEM")?;
    let transform = [r.vec3()?, r.vec3()?, r.vec3()?, r.vec3()?];
    let submesh_count = r.u32()? as usize;
    let bounds_min = r.vec3()?;
    let bounds_max = r.vec3()?;

    r.expect_room(submesh_count, 60)?;
    let mut submeshes = Vec::with_capacity(submesh_count);
    for _ in 0..submesh_count {
        submeshes.push(read_submesh(r)?);
    }
    Ok(Mesh {
        transform,
        bounds_min,
        bounds_max,
        submeshes,
    })
}

fn read_submesh(r: &mut Reader) -> Result<Submesh> {
    r.tag(b"PRGM")?;
    let texture = r.fixed_str(32)?;
    let [red, green, blue, _alpha] = r.u32()?.to_le_bytes();
    r.skip(4)?; // unknown, usually 1
    let vertex_count = r.u32()? as usize;
    let face_count = r.u32()? as usize;
    let lod_count = r.u32()? as usize;
    // Non-zero: the last four vertices are points of their own, which the
    // original leaves out of the submesh's bounds (0x57b140, 0x57ce20).
    let trailing_points = if r.u32()? != 0 { 4 } else { 0 };

    // Positions, normals and UVs: 32 bytes per vertex; 8 bytes per face.
    r.expect_room(vertex_count, 32)?;
    let positions = (0..vertex_count).map(|_| r.vec3()).collect::<Result<_>>()?;
    let normals = (0..vertex_count).map(|_| r.vec3()).collect::<Result<_>>()?;
    let uvs = (0..vertex_count).map(|_| r.vec2()).collect::<Result<_>>()?;

    r.expect_room(face_count, 8)?;
    let mut triangles = Vec::with_capacity(face_count);
    for _ in 0..face_count {
        triangles.push([r.u16()?, r.u16()?, r.u16()?]);
        // A fourth value per face whose meaning nobody has worked out.
        r.skip(2)?;
    }
    if let Some(bad) = triangles
        .iter()
        .flatten()
        .find(|&&i| i as usize >= vertex_count)
    {
        return Err(crate::Error::Format(format!(
            "submesh {texture} indexes vertex {bad} of {vertex_count}"
        )));
    }

    // Level-of-detail blocks: three counts, then arrays of 8-, 4- and 2-byte
    // entries. Contents unknown; skipped by size.
    for _ in 0..lod_count {
        r.tag(b"KDOL")?;
        let (a, b, c) = (r.u32()? as usize, r.u32()? as usize, r.u32()? as usize);
        let size = a
            .checked_mul(8)
            .and_then(|n| n.checked_add(b.checked_mul(4)?))
            .and_then(|n| n.checked_add(c.checked_mul(2)?))
            .ok_or_else(|| crate::Error::Format("LODK block size overflows".into()))?;
        r.skip(size)?;
    }

    Ok(Submesh {
        texture,
        color: [red, green, blue],
        trailing_points,
        positions,
        normals,
        uvs,
        triangles,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(out: &mut Vec<u8>, values: &[f32]) {
        for v in values {
            out.extend(v.to_le_bytes());
        }
    }

    fn one_triangle_model(lod: bool) -> Vec<u8> {
        let mut out = b"LDOM".to_vec();
        out.extend([1, 9, 0, 0]);
        out.extend(1u32.to_le_bytes());
        out.extend([0u8; 4 + 8]); // data size, unknown
        out.extend(2u32.to_le_bytes()); // billboard flag
        out.extend([0u8; 16]);
        out.extend(8u32.to_le_bytes());

        out.extend(b"HSEM");
        f(&mut out, &[1., 0., 0., 0., 1., 0., 0., 0., 1., 5., 6., 7.]);
        out.extend(1u32.to_le_bytes());
        f(&mut out, &[0., 0., 0., 1., 1., 1.]);

        out.extend(b"PRGM");
        let mut name = [0u8; 32];
        name[..7].copy_from_slice(b"GAB_TEX");
        out.extend(name);
        out.extend(0x0030_2010u32.to_le_bytes());
        out.extend(1u32.to_le_bytes());
        out.extend(3u32.to_le_bytes());
        out.extend(1u32.to_le_bytes());
        out.extend((lod as u32).to_le_bytes());
        out.extend(0u32.to_le_bytes());
        f(&mut out, &[0., 0., 0., 1., 0., 0., 0., 1., 0.]);
        f(&mut out, &[0., 0., 1., 0., 0., 1., 0., 0., 1.]);
        f(&mut out, &[0., 0., 1., 0., 0., 1.]);
        for i in [2u16, 1, 0, 0xF100] {
            out.extend(i.to_le_bytes());
        }
        if lod {
            out.extend(b"KDOL");
            for n in [1u32, 2, 3] {
                out.extend(n.to_le_bytes());
            }
            out.extend([0u8; 8 + 8 + 6]);
        }
        out.extend(b"XDOM");
        out
    }

    #[test]
    fn parses_a_model() {
        let model = Model::parse(&one_triangle_model(true)).unwrap();
        assert!(model.billboard);
        assert_eq!(model.meshes[0].transform[3], [5., 6., 7.]);
        let sub = &model.meshes[0].submeshes[0];
        assert_eq!(sub.texture, "GAB_TEX");
        assert_eq!(sub.color, [0x10, 0x20, 0x30]);
        assert_eq!(sub.triangles, vec![[2, 1, 0]]);
        assert_eq!(model.vertex_count(), 3);
    }

    #[test]
    fn misread_counts_move_the_trailer() {
        let mut bytes = one_triangle_model(false);
        // Claim a LOD block that is not there.
        let lod_at = bytes.len() - 4 - 24 - 36 - 36 - 8 - 8;
        bytes[lod_at] = 1;
        assert!(Model::parse(&bytes).is_err());
    }

    #[test]
    fn out_of_range_indices_are_errors() {
        let mut bytes = one_triangle_model(false);
        let index_at = bytes.len() - 4 - 8;
        bytes[index_at] = 9;
        assert!(Model::parse(&bytes).is_err());
    }
}
