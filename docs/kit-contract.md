# parble kit contract

| Field | Value |
|---|---|
| Card | P0-T2a (`plans/phase-0-foundations.md`) |
| Binds | Every piece generator, `parble-core`, `parble-kit`, `parble-sim`, `parble-render`, the editor and procgen |
| Source | `PRD.md` §5.3, §5.4, §5.5 and the reference images in `docs/reference/` |
| Status | Approved by Paul 2026-11-07. All §16 defaults accepted. |
| Contract version | `KIT_CONTRACT_VERSION = 1` |

This document fixes every dimension, tolerance and file format that the kit pieces, the sim and the renderer agree on. A number that a later plan uses must be defined here (see the cross-reference in §15). Changing any number in §2 to §12 is a contract change: bump `KIT_CONTRACT_VERSION`, rebuild the catalog and bump `PHYSICS_VERSION`.

All lengths are in millimetres (mm) in this document. World units in code are metres (60 mm = 0.060 m). Sidecar files store lengths as integer micrometres (µm), see §11.

## 1. Units, axes and the piece-local frame

- **World frame:** right-handed, +Y up (Bevy convention). The grid is axis-aligned with world X, Y and Z.
- **Cell index:** `CellCoord(x, y, z)` in whole cells. Cell `(x, y, z)` spans world `[60x − 30, 60x + 30]` mm in X, `[60y, 60y + 60]` mm in Y and `[60z − 30, 60z + 30]` mm in Z. So cell centres sit on multiples of 60 mm in X and Z, and the bottom face of cell layer `y` is at `60y` mm.
- **Cell layer:** one cell of height (60 mm). "Cell layer" and "port level" (§3) are different units and are never used interchangeably.
- **Faces:** `+X`, `−X`, `+Z`, `−Z` (side faces), `Top` (+Y) and `Bottom` (−Y).
- **Piece-local frame:** the origin is the centre of the bottom face of the piece's anchor cell (§9). Axes match the world axes before orientation is applied.
- **Face-local coordinates for a side port:** `u` is lateral (to the right when looking into the cell from outside), `v` is vertical (up from the cell's bottom face).

## 2. Cell and chunk

| Quantity | Value |
|---|---|
| Cell | 60 mm cube |
| Chunk | 8 × 8 × 8 cells (written 8×8×8 in the plans) = 480 mm cube (used by the collider merge in P1-T3 and the instanced renderer in P1-T4) |
| Visual seam inset | 0.5 mm per face on render meshes, so two neighbouring pieces show a 1.0 mm seam (`ref-02`) |
| Collision seam inset | 0 mm. Collision meshes run exactly to the cell face (§5.4). |

## 3. Horizontal port levels and lateral tolerance

A horizontal (side-face) port is `(cell, face, level)`. The **level** is the height of the channel floor (the lowest point of the round bottom) above the bottom face of that cell, in 15 mm steps.

| Level | Floor height above cell bottom | Absolute floor height in cell layer `y` |
|---|---|---|
| `L0` | 0 mm | `60y + 0` mm |
| `L1` | 15 mm | `60y + 15` mm |
| `L2` | 30 mm | `60y + 30` mm |
| `L3` | 45 mm | `60y + 45` mm |

- There are exactly four levels. A floor at 60 mm is `L0` of the cell layer above, never "L4".
- **Carry rule:** one level below `L0` of cell layer `y` is `L3` of cell layer `y − 1`. One level above `L3` of layer `y` is `L0` of layer `y + 1`. `PortLevel` arithmetic in `parble-core` wraps with this carry.
- **Port position:** a side port's channel centreline is on the face's lateral centre (`u = 0`). Its floor is at `v = 15 × level` mm. Its direction is the face normal.
- **Mating rule:** two side ports mate when they sit on the same shared face (opposite faces of neighbouring cells), have the same absolute floor height and compatible classes (§5.5).

### 3.1 Lateral and vertical tolerance

| What | Tolerance |
|---|---|
| Collision mesh port ring (§5.4) | Exact. The ring vertices are the canonical integer-µm values in §5.4, so mating rings are bit-identical. |
| Chunk-merge vertex weld epsilon | 1 µm (0.001 mm). Vertices closer than this are welded before `FIX_INTERNAL_EDGES` runs. |
| Render mesh at a port (channel centreline, floor height) | ±0.1 mm laterally and vertically |
| Render mesh channel width at a port | 28.0 mm ±0.1 mm |
| Kit-check port position check | Fails if any port's centreline is more than 0.1 mm from `u = 0` or its floor more than 0.1 mm from `15 × level` mm |

Marble play inside the channel (how far a marble can move sideways) is a different quantity. It is in §6.

## 4. Vertical drop port

| Quantity | Value |
|---|---|
| Position | Centre of the `Top` or `Bottom` face (`x = 0`, `z = 0` in the cell) |
| Hole diameter | 32 mm |
| Clearance for the 25 mm shooter | 7 mm on the diameter, 3.5 mm radial |
| Rim chamfer | 2 mm × 45° on the render mesh only |
| Mating rule | A `Bottom` drop port mates with the `Top` drop port of the cell directly below. |

A 32 mm hole is wider than the 28 mm channel, so a drop hole cannot be cut into a plain channel floor without breaking the walls. A channel that ends in a drop hole ends in a **bowl**: a vertical cylindrical pocket of 36 mm diameter centred on the cell centre, with the 32 mm hole through its floor. In the block family the side walls beside the bowl are (60 − 36) / 2 = 12 mm thick. A piece that receives a drop at its `Top` port has a 36 mm funnel mouth narrowing to its channel.

## 5. Channel class and cross-sections

### 5.1 The channel class (shared by both families)

| Quantity | Value |
|---|---|
| Inner width | 28 mm |
| Bottom | Full half-round, radius 14 mm, centred 14 mm above the floor |
| Walls above the half-round | Vertical, at `u = ±14` mm |
| Why radius 14 | Every marble radius (5, 8, 12.5 mm, §6) is below 14 mm, so each marble touches the channel at one point on the centreline rather than riding on two wall contacts. |
| Minimum overhead clearance | 34 mm above the floor (25 mm shooter + 9 mm). Applies to static geometry (tunnels, arches, socket bores) and to animated parts in their open (marble-passing) pose. Animated parts at rest (gate flaps, toggles, trapdoors) intrude into the channel on purpose. |
| Tunnel bore | 28 mm wide, half-round floor and half-round crown, both radius 14 mm, 34 mm tall (6 mm of vertical wall between them) |

### 5.2 Block family (carved U-channel, `ref-01`, `ref-02`, `ref-06`)

The block body is a full 60 mm cube (59 mm rendered after the 0.5 mm seam insets). Open channels are carved down from the top face, so the lip height above the floor depends on the level. Tunnels are bored through the body.

| Quantity | Value |
|---|---|
| Body | 60 mm cube, 59 mm rendered |
| Side wall thickness beside the channel | (60 − 28) / 2 = 16 mm (15.5 mm rendered) |
| Minimum wood under any channel floor | 6 mm |
| Lip radius (fillet where channel wall meets top face) | 3 mm, render mesh only. At `L3` only 1 mm of vertical wall sits under the lip, so the fillet blends into the R14 half-round instead of being tangent to a vertical wall. |
| Outer edge bevel | 1.5 mm, render mesh only |
| Levels a 1-cell open channel can use | `L1`, `L2`, `L3` (floor 15, 30, 45 mm) |
| Levels a 1-cell tunnel can use | `L1` only (crown at 15 + 34 = 49 mm, 11 mm wood above). `L2` would put the crown at 64 mm. |
| `L0` | Not possible inside one cell (0 mm of wood under the floor). A block piece with an `L0` port is 2 cells tall. Its lower cell is either solid body under the floor or holds the piece's exit port (§7, §9). |

Lip height above the floor for a 1-cell open block channel, against each marble:

| Level | Floor | Lip above floor | Mini (Ø10, centre 5) | Standard (Ø16, centre 8) | Shooter (Ø25, centre 12.5) |
|---|---|---|---|---|---|
| `L1` | 15 mm | 45 mm | lip above top | lip above top | lip above top |
| `L2` | 30 mm | 30 mm | lip above top | lip above top | lip above top |
| `L3` | 45 mm | 15 mm | lip above top | **top 1 mm above lip** | **lip 2.5 mm above centre, 10 mm below top** |

`L3` is the showcase level: a standard marble's top sits 1 mm proud of the lip, which matches `ref-02`. `L3` is flagged for the shooter in §16 (Q6).

Block section across the channel at `L3` (the default showcase level), scale about 1 character = 2 mm:

```
        |<-------------------- 60 cell (59 rendered) -------------------->|
        |<---- 16 wall ---->|<------- 28 channel ------->|<---- 16 wall --->|
   60 ->+-------------------.                            .------------------+  <- top face = lip
        |                    \ 3 lip radius   3 lip radius/                  |     lip 15 above floor
        |                     |                          |                   |
        |                     |  vertical wall 1 mm      |                   |
   59 ->|                     |                          |                   |
        |                      \                        /                    |
        |                       \     R14 half-round   /                     |  centre of R14
        |                        '.                  .'                      |  at 45 + 14 = 59
   45 ->|                          '----____----____'  <- floor, level L3     |
        |                                                                    |
        |                     45 mm of wood under the floor                  |
        |                                                                    |
    0 ->+--------------------------------------------------------------------+  <- cell bottom
        (1.5 outer edge bevel on every outer edge, render mesh only)
```

At `L3` the half-round (radius 14) reaches 59 mm, so only 1 mm of vertical wall sits between the half-round and the 3 mm lip fillet. The collision mesh drops the fillet and keeps a sharp lip at 60 mm (§5.4).

Block tunnel section at `L1`:

```
        |<---------------------------- 60 ---------------------------->|
   60 ->+--------------------------------------------------------------+
        |                    11 mm wood above crown                     |
   49 ->|                       .-''''''''''-.   <- crown, R14          |
        |                     /                \                         |
   41 ->|                    |   6 mm vertical  |  (walls at u = ±14)    |
   35 ->|                    |                  |                        |
        |                     \                /                         |
   15 ->|                       '-.________.-'   <- floor, level L1, R14  |
        |                    15 mm wood under floor                      |
    0 ->+--------------------------------------------------------------+
                             |<---- 28 ---->|
        tunnel height 34 = 14 (floor R) + 6 (wall) + 14 (crown R)
```

### 5.3 Rail family (round trough, `ref-05`, `ref-06`)

| Quantity | Value |
|---|---|
| Inner profile | The §5.1 channel: 28 mm wide, R14 half-round bottom |
| Wall thickness | 4 mm everywhere (outer bottom radius 18 mm) |
| Outer width | 28 + 2 × 4 = 36 mm |
| Lip height above floor | 18 mm (R14 half-round plus 4 mm vertical wall) |
| Lip radius | 2 mm full round on top of the 4 mm wall (render mesh) |
| Section envelope | From floor − 4 mm (outer bottom) to floor + 18 mm (lip top), 22 mm tall |
| Levels a 1-cell-tall rail section can use | `L1` (envelope 11 to 33 mm) and `L2` (26 to 48 mm) |
| `L0` and `L3` | Not possible inside one cell layer (`L0` outer bottom at −4 mm, `L3` lip at 63 mm). A rail with an `L0` or `L3` port includes the extra cell layer in its footprint (§9). |
| Shooter in a rail | Lip 18 mm is 5.5 mm above the shooter's centre and 7 mm below its top (§16, Q6) |

Rail section at `L2`, scale about 1 character = 1 mm:

```
             |<------------------ 36 outer ------------------>|
             |<-4->|<------------- 28 inner ------------>|<-4->|
   48 ->     ( 2R )                                       ( 2R )   <- lip, floor + 18, R2 full round
             |    |                                       |    |
   44 ->     |    |   4 mm vertical wall (u = ±14 inside)  |    |   <- top of R14 half-round, floor + 14
             \     \                                     /     /
              \     '.          R14 inner             .'     /
               '.     '-.___________________________.-'    .'
   30 ->         '-.          floor, level L2           .-'         <- inner floor
   26 ->            '-.______________________________.-'            <- outer bottom, R18, floor − 4
```

### 5.4 Collision mesh rules (both families)

- One collision mesh per piece, the same for every LOD.
- The channel half-round is sampled with **16 segments** (17 vertices, every 11.25°). Chord sag is 14 × (1 − cos 5.625°) = 0.067 mm.
- **Canonical port ring:** at every side port the collision vertices on the port plane are exactly the 17 half-round vertices plus the two wall tops, computed in integer µm from `u = 14000 × sin θ`, `v = floor + 14000 − 14000 × cos θ`, `θ = −90° … +90°` in 11.25° steps, rounded half away from zero. `parble-core` exports this ring as a constant table. Kit-check asserts every port ring equals it exactly.
- **Tunnel port ring (P2-T2):** a side port that exits a bored tunnel (the redirect's exit) has no wall tops. Its ring is the same 17 floor vertices plus 17 crown vertices, the floor half-round mirrored about `floor + 17000` µm, so the bore spans `floor … floor + 34000` (§5.1). A socket port's ring sits on its inset plane (`inset_um`, §5.5). Kit-check accepts the open ring or the tunnel ring at each side port. The tunnel ring exists only in `blender/kit/tools/kit.mjs` and is not yet a `parble-core` table.
- Collision floors run to the cell face (0 mm inset). The visible seam gap and bevels exist only on the render mesh.
- Lip fillets and outer bevels are dropped from the collision mesh (sharp edges).
- Wall tops in the collision mesh: block family at the top face (60 mm in a 1-cell piece), rail family at floor + 18 mm. At a block-to-rail mate both walls stand at `u = ±14`, so the marble sees no step, only a lower wall on the rail side.
- **Rail port ring (P2-T4):** a rail's ring is the canonical ring with its two wall tops at `(±14000, floor + 18000)` µm. The rail collision mesh also carries the lip tops and the outer R18 shell (16 segments), which kit-check ignores at a port by looking only within 16 mm of the half-round centre. Rail port ends are left open, as block port faces are.

### 5.5 Port classes and the rail-to-block joint (`ref-06`)

| Class | Mates with | Used by |
|---|---|---|
| `channel` | `channel` | Every block and rail side port. A rail can butt flush against any block channel port, because the collision rings are identical. |
| `socket` | `spigot` | Tower blocks with a round hole (`ref-06`) |
| `spigot` | `socket` | The rail end of the `rail_joint_*` piece |
| `drop` | `drop` | Vertical drop ports (§4) |

The rail-to-block joint is the `ref-06` look: a rail end enters a round hole in a block.

| Quantity | Value |
|---|---|
| Socket bore | 40 mm diameter, centred 14 mm above the port floor (on the half-round centre), spanning floor − 6 mm to floor + 34 mm |
| Socket depth into the block | 10.5 mm |
| Spigot (rail end) length past the rail's own cell face | 10 mm |
| Radial clearance, spigot in socket | 1.88 mm minimum. The farthest point of the rail's outer profile from the bore axis is 18.12 mm (where the 2 mm lip round meets the outer wall), against a bore radius of 20 mm. At the bottom the gap is 2 mm (bore at floor − 6, rail outer bottom at floor − 4). |
| End clearance | 0.5 mm, the standard visual seam |
| Collision port plane | 10 mm inside the socket block's face. The rail's collision floor runs to it, the block's collision channel starts at it, and the rings are the canonical ring. |
| Closed (round) socket | `L1` only in a 1-cell block: bore spans 15 − 6 = 9 mm to 15 + 34 = 49 mm, leaving 9 mm of wood below, 11 mm above and (60 − 40) / 2 = 10 mm beside it. The bore top at floor + 34 meets the §5.1 overhead clearance. |
| Open (U-slot) socket | `L2`, `L3`: the bore would break the top face (`L2` top at 63 mm), so the socket becomes a 40 mm wide U slot from the bore bottom up to the top face |
| Spigot volume | Declared in the rail sidecar as `spigot` and in the block sidecar as `receptacle`. It is the only geometry allowed outside a piece's footprint. Kit-check asserts the spigot volume fits inside the receptacle with at least 1.5 mm radial clearance. |
| As built (P2-T4) | `rail_joint_l1` declares `spigot` = `{axis [1, 0, 0], center_um [30000, 29000, 0], length_um 10000, radius_um 18440}`. The radius covers the collision section's sharp lip corner at (18, 18) mm, 18.44 mm from the bore axis (1.56 mm clear); the render section reaches 18.11 mm (1.89 mm clear). `blk_tower_socket_l1` (P2-T2) declares no `receptacle`, and adding one would change its sidecar bytes, so kit-check holds the spigot radius to the §5.5 bore instead: at most 20 − 1.5 = 18.5 mm. The spigot port's `inset_um` (10000) measures outward, past the rail's own face, so its collision port plane and the socket's coincide and the 17 floor vertices weld. |

Rail-to-block joint, side elevation through the channel centreline (rail at `L1`, block cell on the right), scale about 1 character = 2 mm:

```
                 rail cell (rail_joint_l1)              |  block cell (blk_tower_socket_l1)
                                                         |<------------- 60 ------------->|
   60 ->                                                 +--------------------------------+
                                                         |                                |
   49 ->                                                 |=====.  <- bore top, floor + 34 |
                                                         |     |  socket Ø40              |
   33 ->  _______________________________________________|__   |  rail lip, floor + 18    |
          rail wall (4)                                  |  |  |                          |
   15 ->  ======= rail floor, L1 =========================|==|==|== block channel floor L1 ==>
   11 ->  ~~~~~~~ rail outer bottom, floor − 4 ~~~~~~~~~~~|~~|  |                          |
    9 ->                                                 |=====' <- bore bottom, floor − 6 |
                                                         |                                |
    0 ->                                                 +--------------------------------+
                                                         |<-10->|  spigot reaches 10 mm in
                                                         |<-10.5>| socket depth (0.5 seam)
                                         collision port plane = 10 mm inside the block face
```

### 5.6 Emissive inlays (owner ruling 2026-11-07)

Track channels and plain track pieces (straights, curves, slopes, rails, joints, pillars, supports) carry no emissive inlays or light strips. A light-strip inlay is allowed only as an optional, off-by-default toggle on a mechanical piece (toggle, splitter, one-way gate, one-way hole, checkpoint, hopper, finish). It is never required. Look-dev frame `08-glow-inlay-track-wide.png` is rejected for this reason, and frames 05 and 09 are references for mechanical pieces only.

State indicators (owner request, approved 2026-11-08) are the one inlay that ships on: the checkpoint arch's inlay shows that a marble has passed. It is dark until the first pass, lights with a 0.25 s fade, and holds for the rest of the run. A restarted run starts dark. Indicator inlays are drawn by the renderer (`crates/parble-render/src/inlay.rs`), not modelled in the GLB, so the piece's meshes, collider and bakes are unchanged and kit-check still sees no inlay slot. Their strips are boxes in the piece frame, a fraction of a millimetre proud of a wood face that no neighbour abuts (on the checkpoint, its two side faces), so they stay clear of every port mouth. The on/off state is behaviour state (`inlay/<i>/on`, `inlay/<i>/since`, `crates/parble-sim/src/inlay.rs`), saved and restored with the mechanism, and it never touches physics. The stepper elevator and the P7-T5 decorative inlays use the same primitive. Each inlay group takes a colour preset, warm amber or pale blue. The checkpoint's is warm amber (owner choice, 2026-11-08).

## 6. Marbles, channel clearance and size gates

The PRD fixes the marble sizes as diameters (see §16, Q1).

| Marble | Diameter | Radius | Clearance in 28 mm channel (total / per side) | Passes 12 mm gate | Passes 19 mm gate | Passes 32 mm drop |
|---|---|---|---|---|---|---|
| Mini | 10 mm | 5 mm | 18 mm / 9 mm | Yes, 2 mm total, 1 mm radial | Yes | Yes |
| Standard | 16 mm | 8 mm | 12 mm / 6 mm | No (16 > 12) | Yes, 3 mm total, 1.5 mm radial | Yes |
| Shooter | 25 mm | 12.5 mm | 3 mm / 1.5 mm | No | No (25 > 19) | Yes, 7 mm total, 3.5 mm radial |

- Maximum marble diameter: 25 mm.
- Every marble radius is below the 14 mm channel radius, so contact is single-point on the centreline (§5.1).
- The MVP (P1-T6) uses only the 16 mm standard size in three materials. Mini and shooter arrive in P5.

### 6.1 Size gates

| Gate | Hole diameter | Passes | Blocks |
|---|---|---|---|
| Mini gate | 12 mm | mini | standard, shooter |
| Standard gate | 19 mm | mini, standard | shooter |

Gate holes are round holes in a plate standing across the channel, centred on the channel centreline with the hole bottom on the channel floor. Used by steer play (P8) and the size sieve (P7).

## 7. Slopes

Slope pieces join side ports. Every side port has a level (horizontal) floor tangent at the face, so slopes chain without a gradient-matching rule.

| Slope | Drop per cell | Nominal gradient | Nominal angle | Floor length per cell |
|---|---|---|---|---|
| Gentle | 1 level = 15 mm | 15 / 60 = 25% | 14.04° | 61.85 mm (straight-line, nominal) |
| Steep | 2 levels = 30 mm | 30 / 60 = 50% | 26.57° | 67.08 mm (straight-line, nominal) |

- **Floor profile:** `t = 0 … 1` along the cell, `h` = drop. The gentle slope uses the cubic smoothstep `y(t) = y_in − h × (3t² − 2t³)`: zero gradient at both ports, so the vertical velocity stays continuous across the seam. The steep slope uses the quintic smootherstep `y(t) = y_in − h × (10t³ − 15t⁴ + 6t⁵)` (PRD §5.3, decided 2026-11-08): zero gradient and zero curvature at both ports, so the marble's normal acceleration is continuous too and its seams stay under the seam bump limits.
- **Peak gradient** (at mid-cell): gentle 1.5 × nominal, 37.5% (20.56°); steep 1.875 × nominal, 93.75% (43.15°).
- Gentle slopes descend one port level per cell, steep slopes two, both using the carry rule in §3. Every entry level has a slope, so chains of either kind never dead-end:

| ID | Entry | Exit | Footprint |
|---|---|---|---|
| `blk_slope_gentle_l3` | `L3` | `L2` | 1 cell |
| `blk_slope_gentle_l2` | `L2` | `L1` | 1 cell |
| `blk_slope_gentle_l1` | `L1` | `L0` of the same layer | 1 × 1 × 2 (lower cell is solid body under the floor) |
| `blk_slope_gentle_l0` | `L0` | `L3` of the layer below | 1 × 1 × 2 (exit port in the lower cell) |
| `blk_slope_steep_l3` | `L3` | `L1` | 1 cell |
| `blk_slope_steep_l2` | `L2` | `L0` of the same layer | 1 × 1 × 2 (lower cell is solid body) |
| `blk_slope_steep_l1` | `L1` | `L3` of the layer below | 1 × 1 × 2 (exit port in the lower cell) |
| `blk_slope_steep_l0` | `L0` | `L2` of the layer below | 1 × 1 × 2 (exit port in the lower cell) |

- A 2-tall block piece is two stacked cubes with the channel carved from the top of the upper cube, the same rule as 1-cell pieces, so its channel can be up to 90 mm deep (`blk_slope_steep_l0` exit, floor 30 mm below the upper cube's bottom). See §16, Q5.
- There is no `blk_straight_l0` or `blk_curve90_l0`. A run that reaches `L0` keeps descending through `blk_slope_gentle_l0` or `blk_slope_steep_l0`.
- 25% is far steeper than most real wooden runs. Long gentle chains build speed. P2 sim tests must cover exit speed after a chain of at least 8 gentle slopes (§16, Q4).

## 8. Orientation: yaw and mirror

- Orientation is `Yaw4` (k = 0, 1, 2, 3, meaning k × 90°) plus a `Mirror` flag. Pieces never tip on their sides.
- **Yaw** is a right-handed rotation about +Y through the anchor cell's centre axis. For k = 1 it maps +X to −Z (`Quat::from_rotation_y(π/2)`). Cell offsets transform as `(x, z) → (z, −x)` per quarter turn.
- **Mirror** reflects the piece-local X axis (`x → −x`). **Mirror is applied first, then yaw.**
- A mirrored mesh reverses triangle winding. The loader re-winds mirrored meshes for both render and collision.
- Only chiral pieces (curves, S-rails, splitters with a handed flap, and so on) expose mirror in the editor. For an achiral piece, mirror is equivalent to some yaw and the editor normalises it away.

Side-face mapping for all 8 combinations (`Top` and `Bottom` never change):

| Mirror | Yaw k | +X → | −X → | +Z → | −Z → |
|---|---|---|---|---|---|
| no | 0 | +X | −X | +Z | −Z |
| no | 1 | −Z | +Z | +X | −X |
| no | 2 | −X | +X | −Z | +Z |
| no | 3 | +Z | −Z | −X | +X |
| yes | 0 | −X | +X | +Z | −Z |
| yes | 1 | +Z | −Z | +X | −X |
| yes | 2 | +X | −X | −Z | +Z |
| yes | 3 | −Z | +Z | −X | +X |

Port levels and lateral position (`u = 0`) are unchanged by yaw and mirror.

## 9. Footprints and multi-cell pieces

1. A footprint is a set of integer cell offsets relative to the **anchor cell**, which is the cell holding port index 0. A placement stores the anchor's `CellCoord`, `Yaw4` and `Mirror`.
2. The footprint is exactly the set of cells that the piece's render or collision geometry enters (after the 0.5 mm render inset). Kit-check recomputes it from the meshes and fails on any mismatch.
3. No geometry leaves the footprint, with one exception: a declared `spigot` volume (§5.5).
4. The footprint is face-connected.
5. Every side port lies on a footprint face whose neighbouring cell is outside the footprint. Drop ports lie on `Top` or `Bottom` faces with the same rule.
6. Two placements may not share a cell. The editor's footprint collision check (P3-T1) uses the transformed footprint.
7. Block pieces are 1 cell, except pieces with an `L0` port, slopes that exit into the layer below, and the redirect drop. Those are 2 cells tall (1 × 1 × 2). The lower cell holds either solid body under the floor or the piece's exit port.
8. Rail ends always land on face centres at a port level.
9. A multi-cell piece belongs to the chunk of its anchor cell. Its collision triangles merge into that chunk's trimesh, so a chunk collider may extend past its 480 mm box.
10. Plan-view footprint extent is at most 4 × 4 cells. Height is at most 8 cell layers (one chunk).

MVP multi-cell rails (P2-T4):

| Piece | Footprint (X × Z × Y cells) | Centreline geometry |
|---|---|---|
| Rail straight | 1 × 1 × 1 (longer straights are 1 × N) | Straight, port to port |
| Rail curve 90° | 2 × 2 × 1 | Arc of radius 90 mm about the footprint corner. The entry port is at the centre of the anchor cell's face. Inner rail edge radius 72 mm, outer 108 mm. All 4 cells are entered, so all 4 are in the footprint. Arc length 141.4 mm. As built (P2-T4): cells x 0..1, z −1..0, centre (−30, −90) mm, entry on the anchor's `−x` face, exit on the `−z` face of cell (1, 0, −1), the turn of `blk_curve90_*`. |
| S-rail 1×3 | 2 × 3 × 1 (lateral offset 1 cell over 3 cells length, see §16, Q7). As built (P2-T4) the length runs along +x: cells x 0..2, z −1..0, entry on the anchor's `−x` face, exit on the `+x` face of cell (2, 0, −1) | Two opposed arcs of radius 150 mm, each turning 36.87°: offset 2 × 150 × (1 − cos 36.87°) = 60 mm, length 2 × 150 × sin 36.87° = 180 mm. Edges stay within 12 to 108 mm laterally. Arc length 193.1 mm. |
| Rail-to-block joint | 1 × 1 × 1 plus a 10 mm spigot | Straight rail with a `channel` port and a `spigot` port |
| Helix (P7) | 2 × 2 × N | Defined in P7 against these rules |

## 10. Piece naming

- **Grammar:** `^(blk|rail|mech|terr|music|deco)_[a-z0-9]+(_[a-z0-9]+)*$`, at most 48 characters, ASCII, lower snake case.
- **Pattern:** `<family>_<shape>[_<qualifier>…][_l<level>]`. If the piece has a level-dependent channel, the level suffix `_l0` … `_l3` comes last and names the level of port index 0.
- **Level is a catalog variant, not a runtime parameter:** each legal level is a separate generated piece. The generator emits only the levels that fit (§5.2, §5.3).
- IDs are stable forever, because saved tracks reference them. A geometry change bumps the sidecar's `piece_version`, never the ID.
- Mirror is never part of the ID.

MVP IDs (P1, P2):

| PRD item | IDs |
|---|---|
| Straight | `blk_straight_l1`, `blk_straight_l2`, `blk_straight_l3` |
| Curve 90° | `blk_curve90_l1` … `_l3` (centreline radius 30 mm about the cell corner) |
| Gentle slope | `blk_slope_gentle_l3`, `blk_slope_gentle_l2`, `blk_slope_gentle_l1` (2 tall), `blk_slope_gentle_l0` (2 tall) |
| Steep slope | `blk_slope_steep_l3`, `blk_slope_steep_l2` (2 tall), `blk_slope_steep_l1` (2 tall), `blk_slope_steep_l0` (2 tall) |
| End stop | `blk_end_l1` … `_l3` |
| Drop hole | `blk_drop_l1` … `_l3` |
| Redirect drop | `blk_redirect_l1` … `_l3` (2 tall) |
| Joiner | `blk_join_l1` … `_l3` |
| Support pillar, tower block | `blk_pillar`, `blk_tower_socket_l1`, `blk_tower_socket_l2`, `blk_tower_socket_l3` |
| Hopper, finish | `mech_hopper`, `mech_finish` |
| Splitter, toggle, checkpoint | `mech_splitter_l3`, `mech_toggle_l3`, `mech_checkpoint_l1` (other levels as needed, arches at `L2`/`L3` are 2 tall) |
| One-way gate, one-way hole | `mech_gate_oneway_l1`, `mech_hole_oneway_l1` (arches at `L2`/`L3` are 2 tall, §16 Q13) |
| Stepper elevator (PRD P2 item 17) | `mech_stepper_up1_l3`: in at `L3` on the anchor's `−x` face, out at `L3` on the `+x` face of cell (1, 1, 0), one cell layer up over two cells (footprint 2 × 1 × 2). `mech_stepper_up1_l1`: in at `L1` on the anchor's `−x` face, out at `L3` on the `+x` face of cell (1, 1, 0), the shipped kit's one way from an `L1` port (the hopper's) back up to `L3` (footprint 2 × 1 × 3, y −1…1: its slat slots reach into the layer below the anchor). Other rises and lengths are `mech_stepper_up<n>_l<entry level>` variants of the same generator (`blender/kit/kitstepper.py` `STEPPERS`, which also sets each exit level). Every slat top, fixed and moving, slants down toward the exit (+x) by `SLANT_DEG` = 11.06° (2.5 mm across a 12.79 mm slat), recorded in the sidecar as the part's `slant_mdeg` (11060), so a marble rolls forward onto the next step as soon as its slat is level with it. Derivation: the marble's centre must clear the slat's exit edge, about 7.5 mm from rest against the riser ahead, inside the ~62 ticks (0.26 s) its slat is level with the next step; with Coulomb rolling resistance (14 mm/s² on the level) that needs at least 7.5° at a 2× timing margin, and 11.06° crosses in 9–18 ticks in the sim (`stepper_test.rs`). The stroke is solved so a raised slat's exit edge sits 1 mm above the next fixed step for any slant, never below it. Retune the angle against `docs/reference/ref-12-stepper.*` when that owner reference arrives. |
| Rope-curtain arch | `mech_arch_rope_l1` (1 tall: its underside sits 35 mm above the `L1` floor; `L2`/`L3` would be 2 tall, §16 Q13). The curtain halves the speed of a marble entering it at 0.121 m/s or faster (`ROPE_MIN_HALVING_SPEED_M_S`), measured against the same pass with no curtain. Below that, rolling resistance (14 mm/s²) across the 40 mm zone takes a significant share too, so the marble still gets through but leaves slower than half. A marble launched at 0.05 m/s stops in the zone even with no curtain. |
| Rail family | `rail_straight_l1`, `rail_straight_l2`, `rail_curve90_r90_l2`, `rail_s_1x3_l2`, `rail_joint_l1` |

## 11. Collision and port sidecar

One sidecar per piece: `assets/kit/<id>/<id>.kit.json`, next to `<id>.glb`. `parble-kit` reads it without Bevy or Rapier.

**Byte determinism (P1-T2 `kit-determinism`):** UTF-8, LF line endings, 2-space indent, object keys sorted, arrays in the documented order, no timestamps, no absolute paths, no floats. All lengths are integer µm, all angles integer millidegrees, and axes are axis-aligned integer unit vectors. Collision vertices are emitted in a stable order (by generator construction order, then deduplicated by first occurrence).

| Field | Meaning |
|---|---|
| `schema` | Sidecar schema version (1) |
| `kit_contract` | `KIT_CONTRACT_VERSION` the piece was built against |
| `id`, `piece_version` | §10 |
| `family` | `blk`, `rail`, `mech`, `terr`, `music`, `deco` |
| `chiral` | Whether the editor offers mirror |
| `footprint` | Cell offsets `[x, y, z]` relative to the anchor, sorted |
| `ports` | Ordered list. Index 0 is the anchor port. `kind` is `side` or `drop`. `level` exists only for `side`. `flow` is `in`, `out` or `both`. `inset_um` is 0 except for socket/spigot ports (10000). |
| `collision` | Static collision groups: `surface` id plus flat `vertices_um` (x, y, z triples) and `indices` (triangle triples) in the piece-local frame |
| `sensors` | Trigger volumes for mechanisms: `id`, `shape` (`box` with `center_um` + `half_extents_um`, or `sphere` with `center_um` + `radius_um`) |
| `animated_parts` | One per kinematic part: `name` (the GLB node name, `anim_<name>`), `pivot_um`, `axis` (integer unit vector), `rest_mdeg`, `limits_mdeg`, and its own `collision` list (same shape as the top-level list). A slide part (added 2026-11-08 for the stepper elevator) carries `"joint": "slide"` and `rest_um`, `limits_um` (µm along `axis` from `pivot_um`, unrotated) in place of the two mdeg fields; a part without `joint` is a hinge. Optional `slant_mdeg` (stepper slats): the forward slant of the part's top faces, millidegrees. |
| `spigot` / `receptacle` | Optional cylinder volumes for §5.5: `axis` (integer unit vector), `center_um` (centre of the base), `length_um`, `radius_um` |
| `surface` ids | `wood` (block), `rail` (rail), `wood_end_grain` (gate plates, flaps). They feed `RollSample` and `ContactEvent` in P2-T1. |

Example (one-way gate at `L1`, collision arrays truncated). Port and sensor objects are shown on one line for readability. The generator writes the canonical 2-space-indented form.

```json
{
  "animated_parts": [
    {
      "axis": [0, 0, 1],
      "collision": [
        {
          "indices": [0, 1, 2, 0, 2, 3],
          "surface": "wood_end_grain",
          "vertices_um": [0, 17000, -14000, 0, 51000, -14000, 0, 51000, 14000, 0, 17000, 14000]
        }
      ],
      "limits_mdeg": [0, 85000],
      "name": "anim_flap",
      "pivot_um": [0, 51000, 0],
      "rest_mdeg": 0
    }
  ],
  "chiral": false,
  "collision": [
    {
      "indices": [0, 1, 2],
      "surface": "wood",
      "vertices_um": [-30000, 15000, 0, 30000, 15000, 0, 30000, 15269, 2731]
    }
  ],
  "family": "mech",
  "footprint": [[0, 0, 0]],
  "id": "mech_gate_oneway_l1",
  "kit_contract": 1,
  "piece_version": 1,
  "ports": [
    { "cell": [0, 0, 0], "class": "channel", "face": "-x", "flow": "in", "index": 0, "inset_um": 0, "kind": "side", "level": 1 },
    { "cell": [0, 0, 0], "class": "channel", "face": "+x", "flow": "out", "index": 1, "inset_um": 0, "kind": "side", "level": 1 }
  ],
  "schema": 1,
  "sensors": [
    { "center_um": [-20000, 27500, 0], "half_extents_um": [6000, 12500, 14000], "id": "approach", "shape": "box" },
    { "center_um": [20000, 27500, 0], "half_extents_um": [6000, 12500, 14000], "id": "exit", "shape": "box" }
  ]
}
```

In this example the channel floor is at `L1` (15 mm). The flap hinges about +Z at 51 mm, which is 36 mm above the floor and so clears the 34 mm overhead rule (§5.1). At rest it hangs to 17 mm, 2 mm above the floor, and it swings up to 85° in the allowed direction only. The arch above the pivot stays under the 60 mm cell top, so the piece is 1 cell. In the sim the flap is counterweighted (1 g, closing torque 1e-6 N·m at 20° open, `GATE_CLOSING_TORQUE_NM_AT_20DEG`), and a marble lets itself through only when it carries the energy to swing the flap about 55° open, until the tip clears its top. On a level L1 run that means a cat's eye launched 80 mm before the hinge at 0.065 m/s or faster (`GATE_MIN_LAUNCH_SPEED_M_S`). Slower marbles stop under the flap, and at 0.05 m/s a marble stops there even with no flap. The third static vertex is the first canonical ring vertex after the floor centre (θ = 11.25°: `u` = 14000 × sin 11.25° = 2731 µm, `v` = 15000 + 14000 × (1 − cos 11.25°) = 15269 µm). For a port on an X face the ring's `u` axis is world Z.

## 12. LOD and texel density targets

| Target | Value |
|---|---|
| LOD levels | LOD0, LOD1, LOD2, written LOD0–LOD2 in P1-T2 |
| Triangle budget, 1-cell block piece | LOD0 ≤ 3,000, LOD1 ≤ 1,000, LOD2 ≤ 250 |
| Triangle budget, rail, per cell of centreline length | LOD0 ≤ 1,500, LOD1 ≤ 500, LOD2 ≤ 120 |
| Mechanism pieces | Block budget plus 1,500 / 500 / 120 for all animated parts together |
| Render half-round segments | LOD0 32 (sag 0.017 mm), LOD1 12 (sag 0.12 mm), LOD2 6 (sag 0.48 mm) |
| Collision mesh | One for all LODs, 16 half-round segments (§5.4), ≤ 600 triangles per cell |
| LOD selection | Per chunk (P1-T4), from the projected height of one cell at the chunk's nearest point at 1440p: LOD0 at ≥ 160 px, LOD1 at 48 to 160 px, LOD2 below 48 px |
| LOD transition | Dithered crossfade across ±15% around each threshold (no hard cut) |
| Tileable wood (P1-T1) | 4,096 texels per metre on High (1,024² per 250 mm repeat), which is 245.76 texels across a 60 mm face. Ultra: 8,192 texels per metre (491.52 per face). Low: 2,048 texels per metre. |
| Per-piece baked maps (AO, bevel normal) | 2,048 texels per metre (122.88 texels per 60 mm face). A 1-cell block bakes to a 512² atlas. Multi-cell pieces scale the atlas by footprint, capped at 2,048². |
| Wood variants | 2 or 3 per P1-T1. Each piece instance picks one by a hash of its anchor cell. |

## 13. Diagrams

The dimensioned sections are inline above so every number is searchable:

- Block family: §5.2 (open channel at `L3`, tunnel at `L1`)
- Rail family: §5.3 (`L2`)
- Rail-to-block joint: §5.5 (`L1`, `ref-06`)
- Port levels in one cell (side elevation, `+X` face), scale about 1 character = 2.5 mm:

```
   60 -+-----------------------+  <- L0 of the cell layer above
       |                       |
   45 -|-- L3 ------------->   |  floor height 45
       |                       |
   30 -|-- L2 ------------->   |  floor height 30
       |                       |
   15 -|-- L1 ------------->   |  floor height 15
       |                       |
    0 -+-- L0 ------------->---+  floor height 0 (2-tall pieces only)
       |<-------- 60 --------->|
       side ports at u = 0; drop ports Ø32 at the centre of Top and Bottom
```

- Gentle and steep slope profiles (side elevation; gentle cubic smoothstep, steep quintic smootherstep):

```
   gentle (L3 -> L2)                        steep (L3 -> L1)
   45 ==.                                   45 ==.
         '-.                                       '.
           '-.  peak 37.5% (20.56 deg)               \   peak 93.75% (43.15 deg)
   30         '==                                     \
              |<-- 60 -->|                             '.
                                            15           '==
                                                 |<-- 60 -->|
```

## 14. What kit-check enforces

1. Sidecar parses, schema and `kit_contract` versions match, and the file is byte-identical on rebuild.
2. The ID matches the §10 grammar, and the level suffix matches port 0.
3. Every side port ring equals the canonical ring or the tunnel ring (§5.4), or on a rail piece the rail ring, and the port lies on an outer footprint face (§9). A spigot port's ring sits `inset_um` past the face (§5.5).
4. Render-mesh port position within ±0.1 mm and width 28.0 ±0.1 mm (§3.1).
5. The footprint recomputed from the meshes equals the declared footprint, and no geometry leaves it except a declared spigot. A spigot volume exists exactly when the piece has a spigot port, and its radius clears the §5.5 bore by at least 1.5 mm.
6. The collision mesh is present and within budget, LOD0 to LOD2 are present and within budget (§12). A rail's budget is per cell of centreline length: straight 60 mm, joint 70 mm, curve 141.4 mm, S-rail 193.1 mm.
7. Animated parts named in the sidecar exist as GLB nodes, with pivot and axis present. Since P2-T3 kit-check also requires each mechanism shape's part set (hopper `anim_gate`, splitter and one-way gate `anim_flap`, toggle `anim_rocker`, one-way hole `anim_trapdoor`, stepper `anim_slats`, none on the finish cup or the checkpoint arch), an integer pivot, an axis-aligned unit axis and limits that hold the rest angle, the `anim_<name>` node at the pivot rotated to the rest angle about the axis (a slide part's node unrotated, `rest_um` along the axis from the pivot), `anim_<name>_lod0`–`lod2` children inside the inset footprint, non-empty `wood_end_grain` part collision, and all parts together within the §12 part budget.
8. Overhead clearance of at least 34 mm above every channel floor for static geometry and for animated parts in their open pose (§5.1). Kit-check measures each upward collision hit against the lowest upward hit within ±14 mm across and 14 mm below it, credited as twice the height difference, so a round bore's crown counts from the floor beneath it rather than from the curved bore wall.
9. Provenance file present.
10. No emissive material or inlay slot on any non-mechanical piece, and every mechanical piece's decorative inlay defaults to off (§5.6; state indicators are on).

## 15. Numbers used by later plans (cross-reference)

Every number in `plans/phase-1-look-dev.md` and `plans/phase-2-core-kit.md`, found by `grep -noE '[0-9]…'` over both files, and where it is defined.

| Plan location | Number | Defined in |
|---|---|---|
| P1-T1 | texel density per the kit contract | §12 (4,096 / 8,192 / 2,048 texels per metre) |
| P1-T1 | 2 or 3 wood variants | §12 |
| P1-T1, P1-T5 | Blender 5.2, KTX-Software 4.4.2 | Tool pins, owned by PRD §5.1 |
| P1-T2 | LOD0–LOD2 | §12 |
| P1-T2 | 90° curve | §9, §10 (block curve radius 30 mm, rail curve radius 90 mm) |
| P1-T3, P1-T4 | 8×8×8 chunk | §2 (480 mm) |
| P1-T3 | 20 straights in a row | Test parameter (owned by P1-T3), uses §10 `blk_straight_*` |
| P1-T3 | 2 mm/s seam threshold | Owned by P1-T3. The contract makes it reachable with the exact port ring and 1 µm weld (§3.1, §5.4). |
| P1-T5 | at most 3 shadow-casting lights | Render budget, owned by PRD §5.5 |
| P1-T6 | 16 mm marbles (three types) | §6 (standard, Ø16, radius 8) |
| P1-T6 | IOR 1.5 | Material, owned by P1-T6 |
| P1-T8 | ~40 pieces, 2,000 pieces, 10 marbles, 600 frames, 1440p, p95, load ≤ 15 | Scenario and perf parameters, owned by P1-T8 and PRD §5.9. §12's LOD thresholds are stated at 1440p. |
| P1-T8 | 15 MB gzip, 16.7 ms | Owned by PRD §5.9 and §2 |
| P2 intro, P2-T7 | 16 PRD items | §10 maps every item to IDs |
| P2-T2 | redirect drop "one level lower" | §9 rule 7, §10: one **cell layer** (60 mm) lower, same port level (§16, Q8) |
| P2-T2 | joiner 2 in, 1 out | §10 `blk_join_*`, ports per §3 |
| P2-T4 | 28 mm channel | §5.1 |
| P2-T4 | rail 90° curve | §9 (radius 90 mm, 2 × 2) |
| P2-T4 | S-rail 1×3 | §9 (radius 150 mm, 2 × 3 footprint) |
| P2-T5 | 6 marbles L,R,L,R,L,R | Test parameter, owned by P2-T5 |
| P2-T7 | 10 marbles | Test parameter, owned by P2-T7 |
| P5, P7, P8 (later plans) | mini 10 mm, shooter 25 mm, 12/19 mm gates | §6 |

## 16. Open questions for owner review

Each item is a judgment call made while writing this contract. **Resolved 2026-11-07: Paul approved every default below as written.** Q18 (rail supports) was left open for P2-T4, which decided it (see the row).

| ID | Question | Default in this contract |
|---|---|---|
| Q1 | **Marble size wording conflict.** `plans/phase-0-foundations.md` P0-T2a says "marble radii (mini 10, standard 16, shooter 25 mm)", but PRD §5.3 says "mini 10 mm, standard 16 mm, shooter 25 mm. Max size 25 mm" and the gates are 12 mm and 19 mm holes. The numbers only work as diameters: a 25 mm radius is a 50 mm marble, which cannot fit the 28 mm channel or pass any gate. Fix the plan wording to "diameters"? | Diameters 10 / 16 / 25 mm, radii 5 / 8 / 12.5 mm (§6) |
| Q2 | Drop port diameter | 32 mm (3.5 mm radial clearance for the shooter). A channel ends in a 36 mm bowl around the hole (§4). |
| Q3 | Block body is a full 60 mm cube with channels carved from the top, so lip height falls with level (45 / 30 / 15 mm at `L1` / `L2` / `L3`). The alternative is a fixed lip height with variable body height. | Full cube, which matches the tiled walls in `ref-02` and the tower in `ref-06` |
| Q4 | Slope gradients. Gentle is fixed by the PRD at one level per cell (25%, 14.04°), which is steep for wood. Steep chosen as two levels per cell (50%, 26.57°). Profiles have horizontal ports: gentle cubic smoothstep (peak 1.5 × nominal), steep quintic smootherstep (peak 1.875 × nominal, PRD §5.3, 2026-11-08). Alternatives: gradient-carrying ports for linear chains, or a 2-cell gentle slope (12.5%). | As in §7 |
| Q5 | `L0` ports, slopes that exit into the layer below, and `L3` rails need a 2-cell-tall footprint. A gentle chain therefore uses two 2-tall pieces in every four cells (`_l1`, `_l0`). 2-tall block pieces carve from the top of the upper cube, so channels there run 60 to 90 mm deep. The alternative is to trim the upper cube so the lip follows the floor. There is no straight or curve at `L0`, so a run reaching `L0` must keep descending. | Accepted (§5.2, §5.3, §7, §9) |
| Q6 | Shooter containment. At `L3` the block lip is 15 mm (2.5 mm above the shooter's centre). The rail lip is 18 mm (5.5 mm above). Shooters may jump curves at speed. | Kept. P2 sim tests must run a shooter at max entry speed through `L3` curves and rail curves. If it escapes, shooter-rated routes use `L1` / `L2` blocks. |
| Q7 | "S-rail 1×3" read as a lateral offset of 1 cell over 3 cells of length, which needs a 2 × 3 footprint. A 1-wide S can only be a vertical S, since ports sit at face centres. | Plan-view S, 2 × 3, radius 150 mm (§9) |
| Q8 | P2-T2's redirect drop "exits sideways one level lower". A 15 mm port level is not a drop. | Read as one cell layer (60 mm) lower, same port level, 1 × 1 × 2 footprint |
| Q9 | Level as a catalog variant (a separate ID per level) rather than a runtime parameter. This multiplies the catalog by up to 3. | Variant (§10), so every collision mesh is generated and kit-checked |
| Q10 | Wall thickness and lip radius: block side walls 16 mm, wood under floor at least 6 mm, lip fillet 3 mm, edge bevel 1.5 mm. Rail wall 4 mm, lip 18 mm above floor, lip 2 mm full round. | As in §5.2, §5.3 |
| Q11 | Rail-to-block joint uses a separate `rail_joint_*` piece with a 10 mm spigot into a 40 mm socket (1.88 mm minimum radial clearance, bore top at floor + 34 to meet the overhead rule). The spigot is the only geometry allowed outside a footprint. Plain rails also butt flush against any block port. | As in §5.5 |
| Q12 | Tolerances: exact integer-µm canonical port ring, 1 µm weld epsilon, ±0.1 mm on render meshes, 0.5 mm visual seam inset per face. | As in §3.1 |
| Q13 | Overhead clearance and tunnel height 34 mm (shooter + 9 mm). This rules out 1-cell `L2` tunnels and pushes `L3` arches (checkpoint, one-way gate) out of the cell top, so those mechanisms need 2-tall footprints or lower levels. | As in §5.1 |
| Q14 | LOD triangle budgets (3,000 / 1,000 / 250 per block, 1,500 / 500 / 120 per rail cell), switch thresholds (160 px and 48 px cell height at 1440p) and ±15% crossfade. Worst case is 6 M triangles if all 2,000 stress pieces sit at LOD0, so P1-T8 validates the thresholds. | As in §12 |
| Q15 | Texel density: tileable wood at 4,096 texels per metre on High, 8,192 on Ultra, 2,048 on Low. Baked per-piece maps at 2,048 texels per metre (512² per 1-cell piece). | As in §12 |
| Q16 | Rail curve radius 90 mm on a 2 × 2 footprint. Block curve radius 30 mm inside 1 cell (inner wall radius 16 mm, tight for a shooter). | As in §9, §10 |
| Q17 | Sidecar stores integer µm and millidegrees rather than floats, for byte-identical rebuilds and exact port rings. | As in §11 |
| Q18 | Rail supports: rails carry no posts in this contract. How rails are held up (integrated posts, or `blk_pillar` under the rail) is left to P2-T4. | Decided in P2-T4: the rail pieces carry no integrated posts. A rail is held by the blocks it joins (`ref-05`, `ref-06`) and, where a run needs it, by a separate support piece under it, which a later card adds. |
| Q19 | Multi-cell pieces merge into their anchor cell's chunk collider, so a chunk collider can extend past 480 mm. Seams between chunk colliders are not covered by `FIX_INTERNAL_EDGES`. | P1-T3 measures the chunk-boundary seam with the same 2 mm/s metric |
