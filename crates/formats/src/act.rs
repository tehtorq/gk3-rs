//! Vertex animations (`.ACT`): every movement in the game.
//!
//! GK3's characters have no skeleton. A clip records, per frame and per mesh,
//! the mesh's transform and where each of its vertices is. Meshes that do not
//! move on a frame are not written again, so each track is a list of poses
//! at the frames they changed. Layout follows G-Engine's
//! `VertexAnimation::ParseFromData`; `ACTH` is written `HTCA`. Playback is
//! 15 frames a second.

use crate::reader::Reader;
use crate::{Error, Result};

pub const FRAMES_PER_SECOND: f32 = 15.0;

#[derive(Debug, Clone, PartialEq)]
pub struct VertexAnimation {
    /// The model this animates, from the header. The file name is not a
    /// reliable guide: one clip in eight is filed under another name.
    pub model: String,
    pub frame_count: u32,
    pub meshes: Vec<MeshTrack>,
}

/// Everything recorded for one mesh of the model.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MeshTrack {
    /// Mesh-to-model transforms (i, j, k, position columns) by frame.
    pub transforms: Vec<Keyed<[[f32; 3]; 4]>>,
    /// Vertex positions by submesh, then by frame.
    pub submeshes: Vec<Vec<Keyed<Vec<[f32; 3]>>>>,
}

/// A value recorded on a frame, holding until the next one.
#[derive(Debug, Clone, PartialEq)]
pub struct Keyed<T> {
    pub frame: u32,
    pub value: T,
}

impl VertexAnimation {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader::new(bytes);
        r.tag(b"HTCA")?;
        let version = r.u32()?;
        if version != 258 {
            return Err(Error::Unsupported(format!("ACT version {version}")));
        }
        let frame_count = r.u32()?;
        let mesh_count = r.u32()? as usize;
        r.skip(4)?; // payload size
        let model = r.fixed_str(32)?;
        r.expect_room(frame_count as usize, 4)?;
        let offsets: Vec<u32> = (0..frame_count).map(|_| r.u32()).collect::<Result<_>>()?;
        r.expect_room(mesh_count, 6)?;

        let mut meshes = vec![MeshTrack::default(); mesh_count];
        for (frame, &offset) in offsets.iter().enumerate() {
            let frame = frame as u32;
            if r.position() != offset as usize {
                return Err(Error::Format(format!(
                    "frame {frame} starts at {} but the table says {offset}",
                    r.position()
                )));
            }
            for (index, track) in meshes.iter_mut().enumerate() {
                let mesh = r.u16()? as usize;
                if mesh != index {
                    return Err(Error::Format(format!(
                        "frame {frame}: expected mesh {index}, found {mesh}"
                    )));
                }
                let mut remaining = r.u32()? as usize;
                while remaining > 0 {
                    let start = r.position();
                    read_block(&mut r, frame, track)?;
                    let used = r.position() - start;
                    remaining = remaining.checked_sub(used).ok_or_else(|| {
                        Error::Format(format!(
                            "frame {frame} mesh {index} overruns its byte count"
                        ))
                    })?;
                }
            }
        }

        // A fifth of retail files end with five unexplained bytes.
        if r.remaining() != 0 && r.peek_rest() != [1, 0, 0, 0, 0] {
            return Err(Error::Format(format!(
                "{} unexpected bytes after the last frame",
                r.remaining()
            )));
        }
        Ok(Self {
            model,
            frame_count,
            meshes,
        })
    }

    pub fn duration_seconds(&self) -> f32 {
        self.frame_count as f32 / FRAMES_PER_SECOND
    }
}

fn read_block(r: &mut Reader, frame: u32, track: &mut MeshTrack) -> Result<()> {
    let id = r.u8()?;
    let size = r.u32()? as usize;
    let body_start = r.position();
    match id {
        0 | 1 => {
            let submesh = r.u16()? as usize;
            let count = r.u16()? as usize;
            if track.submeshes.len() <= submesh {
                track.submeshes.resize(submesh + 1, Vec::new());
            }
            let poses = &mut track.submeshes[submesh];
            let positions = if id == 0 {
                r.expect_room(count, 12)?;
                (0..count).map(|_| r.vec3()).collect::<Result<Vec<_>>>()?
            } else {
                // Deltas against the previous recorded pose, which must
                // exist and agree on the vertex count.
                let previous = poses
                    .last()
                    .map(|p: &Keyed<Vec<[f32; 3]>>| &p.value)
                    .filter(|p| p.len() == count)
                    .ok_or_else(|| {
                        Error::Format(format!(
                            "frame {frame}: compressed submesh {submesh} has no previous pose"
                        ))
                    })?;
                read_compressed(r, previous)?
            };
            poses.push(Keyed {
                frame,
                value: positions,
            });
        }
        2 => {
            if size != 48 {
                return Err(Error::Format(format!("transform block of {size} bytes")));
            }
            let value = [r.vec3()?, r.vec3()?, r.vec3()?, r.vec3()?];
            track.transforms.push(Keyed { frame, value });
        }
        3 => {
            if size != 24 {
                return Err(Error::Format(format!("bounds block of {size} bytes")));
            }
            r.skip(24)?;
        }
        other => return Err(Error::Format(format!("unknown ACT block type {other}"))),
    }
    // Trust the block's own size over what was read from it.
    if r.position() != body_start + size {
        return Err(Error::Format(format!(
            "block type {id} at {body_start} declared {size} bytes, read {}",
            r.position() - body_start
        )));
    }
    Ok(())
}

/// Two bits a vertex, low bits first: 0 unchanged, 1 byte deltas, 2 short
/// deltas, 3 float deltas.
fn read_compressed(r: &mut Reader, previous: &[[f32; 3]]) -> Result<Vec<[f32; 3]>> {
    let count = previous.len();
    let codes = r.bytes(count / 4 + 1)?.to_vec();
    let mut out = Vec::with_capacity(count);
    for (k, p) in previous.iter().enumerate() {
        let code = (codes[k / 4] >> ((k % 4) * 2)) & 3;
        let delta = match code {
            0 => [0.0; 3],
            1 => [
                byte_delta(r.u8()?),
                byte_delta(r.u8()?),
                byte_delta(r.u8()?),
            ],
            2 => [
                short_delta(r.u16()?),
                short_delta(r.u16()?),
                short_delta(r.u16()?),
            ],
            _ => r.vec3()?,
        };
        out.push([p[0] + delta[0], p[1] + delta[1], p[2] + delta[2]]);
    }
    Ok(out)
}

/// Sign bit, two whole bits, five fraction bits.
fn byte_delta(b: u8) -> f32 {
    let sign = if b & 0x80 != 0 { -1.0 } else { 1.0 };
    sign * (((b & 0x7F) >> 5) as f32 + (b & 0x1F) as f32 / 32.0)
}

/// Sign bit, seven whole bits, eight fraction bits.
fn short_delta(v: u16) -> f32 {
    let sign = if v & 0x8000 != 0 { -1.0 } else { 1.0 };
    sign * (((v & 0x7FFF) >> 8) as f32 + (v & 0xFF) as f32 / 256.0)
}

/// The value in force at `frame`: the last recorded at or before it.
pub fn at<T>(track: &[Keyed<T>], frame: u32) -> Option<&Keyed<T>> {
    let i = track.partition_point(|k| k.frame <= frame);
    i.checked_sub(1).map(|i| &track[i])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_point_deltas() {
        assert_eq!(byte_delta(0b0010_0000), 1.0);
        assert_eq!(byte_delta(0b1001_0000), -0.5);
        assert_eq!(byte_delta(0b0110_1000), 3.25);
        assert_eq!(short_delta(0x0180), 1.5);
        assert_eq!(short_delta(0x8040), -0.25);
    }

    /// One mesh, one submesh of two vertices: raw on frame 0, a transform on
    /// frame 0, compressed deltas on frame 1 (vertex 0 by bytes, vertex 1
    /// unchanged).
    fn clip(trailer: bool) -> Vec<u8> {
        let mut frame0 = Vec::new();
        frame0.extend(0u16.to_le_bytes());
        let mut blocks = Vec::new();
        blocks.push(0u8);
        blocks.extend((4u32 + 24).to_le_bytes());
        blocks.extend(0u16.to_le_bytes());
        blocks.extend(2u16.to_le_bytes());
        for v in [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0] {
            blocks.extend(v.to_le_bytes());
        }
        blocks.push(2u8);
        blocks.extend(48u32.to_le_bytes());
        for v in [1.0f32, 0., 0., 0., 1., 0., 0., 0., 1., 10., 20., 30.] {
            blocks.extend(v.to_le_bytes());
        }
        frame0.extend((blocks.len() as u32).to_le_bytes());
        frame0.extend(&blocks);

        let mut frame1 = Vec::new();
        frame1.extend(0u16.to_le_bytes());
        let mut block = vec![1u8];
        let body: Vec<u8> = [
            &0u16.to_le_bytes()[..],
            &2u16.to_le_bytes(),
            &[0b0000_0001],
            &[0b0010_0000, 0b1001_0000, 0],
        ]
        .concat();
        block.extend((body.len() as u32).to_le_bytes());
        block.extend(body);
        frame1.extend((block.len() as u32).to_le_bytes());
        frame1.extend(&block);

        let header_len = 52 + 8;
        let mut out = b"HTCA".to_vec();
        out.extend(258u32.to_le_bytes());
        out.extend(2u32.to_le_bytes());
        out.extend(1u32.to_le_bytes());
        out.extend(0u32.to_le_bytes());
        let mut name = [0u8; 32];
        name[..3].copy_from_slice(b"GAB");
        out.extend(name);
        out.extend((header_len as u32).to_le_bytes());
        out.extend(((header_len + frame0.len()) as u32).to_le_bytes());
        out.extend(frame0);
        out.extend(frame1);
        if trailer {
            out.extend([1, 0, 0, 0, 0]);
        }
        out
    }

    #[test]
    fn parses_raw_compressed_and_transform_blocks() {
        let act = VertexAnimation::parse(&clip(true)).unwrap();
        assert_eq!(act.model, "GAB");
        assert_eq!(act.frame_count, 2);
        let track = &act.meshes[0];
        assert_eq!(track.transforms[0].value[3], [10.0, 20.0, 30.0]);
        let poses = &track.submeshes[0];
        assert_eq!(poses.len(), 2);
        assert_eq!(poses[1].frame, 1);
        assert_eq!(poses[1].value, vec![[2.0, 1.5, 3.0], [4.0, 5.0, 6.0]]);
        assert_eq!(at(&track.transforms, 1).unwrap().frame, 0);
    }

    #[test]
    fn stray_trailing_bytes_are_errors() {
        let mut bytes = clip(false);
        bytes.push(7);
        assert!(VertexAnimation::parse(&bytes).is_err());
    }

    #[test]
    fn misplaced_frames_are_errors() {
        let mut bytes = clip(false);
        bytes[56] += 1; // frame 1's offset
        assert!(VertexAnimation::parse(&bytes).is_err());
    }
}
