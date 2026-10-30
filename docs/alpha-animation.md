# The 2013 alpha's animation clips

How the alpha keeps its animation, what `crates/g2-frostbite/src/ant.rs` decodes, how the
importer turns it into glTF clips and how the client plays them. Everything below was read
from, or measured on, the October 2013 closed-alpha client ("Generals2 F2P"); counts are
that build's.

## Where the clips live

One ebx `AntProjectAsset` (`_rts/art/animation/masteranimations`) names one
`AntPackageAsset`, `s_CNCO_Common` (packaging type 0, static). Its data is the res
`_rts/art/animation/masteranimations/s_cnco_common`, res type `0x51A3C853` (29 MB
decompressed). Blueprints reach the package's objects through `AntRef { AssetId }` fields
(their actors, state graphs and bindings); `RtsAntAnimatableComponentData` pairs a
`MasterSkeletonAsset` (`*_mskl`, whose `MasterSkeleton` is the `SkeletonAsset`) with the
meshes it animates.

## Container: EA GenericData

- Big-endian `u32` packaging type, `u32` header size, then sections: `char[7]` tag, an
  endianness byte (`l` in this build), `u32` size (including the 16-byte header), `u32`.
- `GD.STRM` (16 bytes, no body), one `GD.REFL`, then one `GD.DATA` per object: 24,202.
- `GD.REFL` describes every class: a `u64` count, an `i64` offset per layout. A layout is
  `i32` min and max slot, `u32` size, alignment, string-table offset and length, two flag
  bytes, two pad bytes, `u32` hash; then 32 bytes per slot (`u32` layout hash, element size,
  offset, name offset; `u16` count, flags (bit 0: array), element alignment; `i16`; `i64`
  offset of the field's own layout); a pad byte and the class name. A slot with element size
  or count 0 is unused.
- A `GD.DATA` body starts with an object header: the layout hash at +16 and the field offset
  at +28 (`u16`). Strings and arrays are `{u32 capacity, u32 count, i64 offset}`, offsets
  from the body. `__base` (a `DataRef`) points at a second header for the base class's part.
- Object references are 16-byte `Guid` fields; the first four bytes are the object index.

Sources: marv7000/AssetBankPlugin (GPL-3.0) for the section and layout headers and the
openBF3 project's ANT notes; both checked against the alpha's bytes. No code was taken.

## Clips

1,700 `ClipControllerAsset`s each play one clip: 1,683 `VbrAnimationAsset` (codec
`"VBR "`) and 17 `RawAnimationAsset`. The controller's name is the clip's
(`EU_RifleSoldier_AtkA`), with its `FPS` (30 for all but 12) and `Target`, a
`LayoutHierarchyAsset` whose `LayoutAssets` list the DOFs in order: each `LayoutAsset` slot
is `<Joint>.q` (type 14, rotation), `.t` (`0x7A2E5497`, translation) or `.s`
(`0x7A2E53C6`, scale); a `DeltaTrajLayoutAsset` takes 8 unnamed DOFs. The clip's
`ChannelToDofAsset` maps channel `c` to DOF `IndexData[c]` (a byte each, or a big-endian
`u16` each for storage type 2). Channels are quaternions, then vectors, then floats.
Checked on all 1,700: every channel lands on a DOF of its own kind.

### VBR

`Data` holds, in order:

1. Key times (`KeyTimeSize` bytes): run lengths alternating keyed and skipped frames,
   starting keyed (no runs: one key per frame). They sum to `NumKeys` keyed frames over
   `EndFrame + 1`, in all 1,683 clips, and agree with the FIFA 17 fork's decompiled seek
   routine on all 209,819 frames.
2. Constant channels: a palette index per component (4 per quaternion, 3 per vector, 1 per
   float) into `ConstantPalette`, each scaled to its kind's `Min..Max`.
3. The constant-channel map (`ConstChanMapSize` bytes): run lengths alternating animated
   and constant channels, starting animated.
4. Per animated scalar, a `u32` of eight 4-bit coefficient widths.
5. The frame blocks (`FrameBlockSizes`), 8 keys each.

A block: three quantisation bytes (rotations, trajectory, other vectors), then the rest,
stored byte-reversed and read LSB-first. Per scalar: a presence bit for each coefficient of
non-zero width, a sign bit for each present one (1 = positive), then the magnitudes.
Coefficient `k` is multiplied by `(dct · 0.2 · (w + 1) · ln(k + 2) + 1) · dct / 32768`
(`w` the block's byte for the scalar's kind, `dct` the clip's `Dct`, 4.0 here); the 8-point
inverse DCT (`1/8` for the DC term, `cos(π(2r + 1)k / 16) / 4` otherwise) plus 0.5 gives a
value in 0..1 that the kind's `Min..Max` scales. Vectors take four lanes (the fourth
unused). A last block of fewer than 8 keys holds the clip's last 8 keys, overlapping the
block before it.

Checked on every clip: each of the 14,227 blocks decodes using exactly the bits it holds;
animated quaternions come out within 0.01% of unit length at the median and 0.4% at the
99th percentile, before normalising; the overlapping last block makes the step into it the
size of the clip's other steps (median ratio 1.00 over 400 clips, 1.72 read as the block's
first keys). No clip uses the offset ramps (`VectorOffsetSize`, `FloatOffsetSize`).

Sources: two independent GPL-3.0 decoders that agree, 4FingerStudios/FrostyBlender's
`tools/ant.py` and the FIFA 17 Frosty fork's `FrostyAnimationEditor/Core/Anim/VbrClip.cs`.
Written from their description and checked against the alpha; no code was taken.

### Rigs and root motion

169 `RigAsset`s, each with an ANT `SkeletonAsset` (joint names and parents). A rig's clips
are the controllers whose target lies in its tree of `LayoutHierarchyAsset`s. The importer
gives a mesh's ebx skeleton the rig sharing at least 60% of its joint names with the same
parents (most share all of them). The infantry rig (`MASTER_Infantry_SKL`, 50 joints, 240
clips) is the EU rifle soldier's skeleton, which every rifle and rocket soldier's blueprint
uses: one skeleton animates the APA and GLA soldiers too.

The rigs' `AITrajectory` joint carries a quarter turn about Y that the ebx skeletons' bind
poses lack; the joints below it carry the inverse. The importer folds the trajectory's turn
into its children: over 475 such joints of every rig, the clips' first keys then sit a
median 16 degrees from the bind pose, against 90 unfolded. `AITrajectory`'s translation is
root motion, the distance the clip carries the unit (4.15 units per `RifleShared_RunFwd`
cycle); it is left out of the clips and kept as the clip's stride.

## Import and playback

`g2-tools alpha DIR import` (or the menu's importer) writes, per animated skeleton,
`animations/<skeleton>.glb` (its joints as TRS nodes and each clip as a named glTF
animation, mirrored in X like the models, joints a clip leaves out held at their bind pose,
plus a last `BindPose` clip) and `animations/<skeleton>.txt` (per clip: index, seconds,
stride, name), and `animations/index.txt` (model, skeleton). Skinned models: 361 of 2,154
(211 before the blueprints' skeleton links skinned the infantry); 156 skeletons, 2,013 clips;
132 MB.

The client (`rig.rs`) plays one clip per model, chosen by what its sim entity does
(`alpha::clip_for`: idle, moving, attacking or working, dying), its time computed from the
sim tick and a phase per entity and squad slot, so a frame is the same on every run. Runs
play at the unit's pace over the clip's stride. Helicopter propellers turn in code: the
hover clips leave them still.

## Not used

- The ANT state graphs (`ActorAsset`, `StateFlow*`, blend spaces, `BlendArray`): the
  client picks clips by name instead.
- Additive and layered clips (`*_Layered`, `AimUp`/`AimDown`), turn-in-place (`StndTo90`),
  hit reactions (`Hit*`), building construction (`*Bld_BldA`), door and production clips.
- Rig ops (twist joints, IK): the infantry's skin has none that matter at RTS distance.
- `CurveAnimationAsset`, `DctAnimationAsset`, `FrameAnimationAsset`: none in this build.
