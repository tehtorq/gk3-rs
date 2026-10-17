# Where gk3-rs differs from the original

gk3-rs aims to play as Gabriel Knight 3 did. Where it does not, that is
either a decision, recorded here with its reason, or a gap still to be
closed, listed at the end. Each entry says how we know what the original
does:

- **traced**: read in the original executable (notes in `docs/re-*.md`);
- **inferred**: from the original's data, options or art, not its code;
- **G-Engine**: what the G-Engine reimplementation does, not yet checked
  against the original.

## Deliberate differences

### Walking

**The end of a walk eases in.** *(traced: `re-walking.md`)* The original
puts the walker exactly on the spot and in the standing pose in one frame
(G-Engine does the same, noting that the original never plays the
`StopAnim` walk-end animations). That shows as a small jump: up to the
walk's arrival distance in position, and from mid-stride to standing in
pose. Ours glides the last few units and blends the pose into standing over
a quarter of a second (`Settle` in `cast.rs`). The standing model is turned
so the body's direction carries straight on, and the pose it stopped in is
turned to match before the two are blended, so the body neither swings round
nor slides as it settles. The walker still ends exactly where the original
would.

**Exact walks that start close by are walked.** *(traced)* When a script's
walk to an exact spot (`WalkTo`, `WalkToXZ`, a fidget's walk, `ClearRegion`)
starts within 24 units of it, the original does not walk: it moves the
walker onto the spot and heading at once (0x41dadd → 0x41826b). Ours walks
there, ending as the original would, because the jump is visible and the
result the same.

### Interface

**Door sounds.** *(inferred)* The original plays no sound when the player
goes through a door to another room. Ours plays the door's sound, on by
default; the Game options panel has a switch for it, in the original
panel's style.

**Graphics options that cannot change are shown fixed.** *(traced: the
option names are the original's)* Texture Quality and Lighting show "High"
(the original's word for its best, from the executable's own strings), as
ours always draws full-size textures and full lighting. Gamma, Incremental
Rendering and LOD are shown unavailable in the original's own disabled art,
as G-Engine leaves them.

### Rendering

These go beyond what the original's options offered, on hardware it never
had; none of them softens textures.

- **4x multisampling** on a real GPU (none on a software renderer), smoothing
  polygon edges only. *(inferred: the original's options have no
  anti-aliasing)*
- **16x anisotropic filtering** with mipmaps and trilinear filtering on,
  keeping floors and walls seen at a slant sharp. *(inferred)*
- **Sharper mipmaps**: each level made with a Lanczos filter in linear
  light rather than averaged.
- **A character the camera is inside is drawn one-sided** (back faces
  culled), so the camera sees out through them rather than their face
  inside out. Otherwise characters are two-sided, as the original draws
  them: some of their triangles face inwards (Gabriel's jeans), and culling
  them shows as holes. Props stay two-sided, as their leaf cards face either
  way. Rooms are drawn
  one-sided, clockwise in front, as G-Engine draws them. *(G-Engine)*
- **Widescreen**: the view keeps the original's vertical field of view and
  widens to the window, so wider windows show more at the sides than the
  scenes were framed for. Full-screen pictures and the interface keep their
  shape, centred, with bars.

### Lighting of characters and props

*(traced: 0x4b05e9, 0x4b07e9, 0x4b08a5, 0x4b0546, 0x5003b4, 0x558c80,
0x559550, 0x4d6810, 0x4adf4d, 0x559fa0, 0x579660)* As the original:

- A model lit evenly (`fullLighting`: half white; `fullColor`: its numbers
  over 512; `SetModelLighting`: likewise, with its range) keeps that colour.
  Any other takes the light from the walk floor: the lightmap colour
  straight below the middle of its bounds and twelve points about it (two
  hexagons, 12 and 24 out), averaged; half until taken; taken again once it
  has moved a unit across the floor, and eased in over four frames. It is
  taken each frame just before the model is drawn.
- A vertex is its tint times (the colour plus (2 cos - 1) times the range),
  cos the light on the surface's front (none behind), clamped to 0 to 1; the
  range is `ambientRange`, 0.3 by default; the light runs from the room's
  `globalLight` to the middle of the model.
- The original's defaults (model shading "Diffuse", "2x lighting" on) on a
  card that can modulate twice over, as any now can: the colour is capped at
  1, the range halved, and the texture modulated by the vertex twice over.
  (A card that could not had the colour and range doubled instead and
  modulated once, to the same effect.)

Where ours differs: four of our frames are shorter than four of the
original's. And a model's first light is taken outright, not eased in from
half: the original eases it in, so on entering a room a model on a dark
floor (the lobby's front door) shows lighter for its first frames before
darkening. Ours is lit right from the first frame, as the original itself
does when it restores a game (0x4b050b fills the four at once).

### Shadows

*(traced: 0x49b584, 0x4b05e9, 0x545430, 0x57ce20, 0x57cfd0, 0x599f20,
0x59ae00, 0x579c20, 0x5938e0, 0x564f50, 0x50d8a6, 0x4d6810)* As the
original, which starts with blob shadows (its "model" shadows are only
switched on by a script command no script uses):

- Characters cast one by default, props when their line says `shadow`
  (the later of `shadow` and `noShadow` holds; Gabriel's moped).
- A blob lies on the walk floor straight below the middle of the model's
  bounds, half a unit above it, as wide and deep as the bounds, and is not
  drawn where no floor lies below. The bounds are each showing mesh's box
  about its showing parts' vertices (less the four trailing points some
  parts carry), a tenth larger, carried into the world.
- It is `SHADOW` (or the model's own shadow texture) doubled and multiplied
  onto the picture, as the original's renderer does on a card that can
  modulate twice over.
- Surfaces of the room flagged as shadows (64; the moped's in MOP, and
  others in RC1, ARM, CDB, LHE, PLO, TE5) are drawn after the rest of the
  room, without a lightmap, their texture multiplied onto the picture, not
  written to depth and pulled a little nearer.

### Camera

**Corners and creases hold the camera still.** *(G-Engine)* The camera
moves as a sphere that stops at the first wall in its way and slides along
it, as G-Engine's. Ours also slides only along the crease where two walls
meet, and never moves back against the push, because otherwise the camera
swings between the two every frame. The original's own camera code is still
to be traced.

## Known differences and approximations

Behaviour that is not yet the original's, with what is known.

| What | The original | Ours | Status |
|---|---|---|---|
| Where a hidden actor posed by its `initAnim` stands | A walker that is not walking follows its model each frame, but only while the model is visible *(traced: 0x41838e → 0x4d6d81 → 0x417e56)*: a hidden actor's walker stays where it was made (its `pos=`, else the origin) until shown | Stood where the posed model is at once, shown or not | — |
| Sound while skipping | Sound types 1 and 8 are switched off while the story is run on *(traced: 0x4b18a5; which sounds those types are, not traced)* | Speech and sound effects do not start; music and the rooms' ambient sound go on | To trace the types |
| The GPS's size | Picked by the screen's width (1024, 800) and drawn unscaled *(traced: 0x46f7ca)* | Picked by the window's height as a 4:3 screen (768, 600) and scaled to the window, as the rest of the interface | — |
| TE3's pendulum | The `Circle` scene's own code *(clicks traced: `re-pendulum.md`)* | G-Engine's `Pendulum` for the motion and timing; the clicks as traced | To trace the motion (0x457eed) |
| The order TE5's stones glint in | Shuffled each round with the C library's `rand()` *(traced)*; how it was seeded is not | Shuffled each round from a sequence of our own | — |

### Small differences in what is traced

- **A morph's easing** (`StartMorphAnimation`): the original moves each
  vertex in a straight line from where it was to the animation's frame;
  ours mixes each mesh's placement and its vertices, which over the
  quarter-second the game asks for looks the same. *(traced:
  `re-animation.md`)*
- **Sounds placed in the room** are heard as Wine's DirectSound hears the
  original's 3D sounds (`re-sound.md`): its formulas for the fall-off and
  the share between the speakers. Windows' own DirectSound, or a sound
  card's, may place them differently. *(traced in the original; the
  hearing is Wine's)*

## Followed from G-Engine, not yet checked

These follow G-Engine's reading of the original and are believed right,
but have not been traced in the executable yet:

- A script's walk that the walk map cannot finish goes as near as it can,
  then on to the spot (`mustReachDestination`).
- Characters' normals are stored as their meshes rest, in model space;
  they are taken back into each mesh's space and turned with the mesh as
  animated, so the light on a character follows its animation.
- `IsActorNear` measures in three dimensions, from the actor's floor point
  raised by its shoes.
- Skip into view: a walk starting off camera begins just before the route
  comes into view. *(traced: for `WalkTo` only for the ego; for clicks by
  default)*
