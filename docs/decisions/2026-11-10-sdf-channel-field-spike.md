# Spike: one signed distance field for straights, bends and slopes (2026-11-10)

Status: trial alongside the current contacts, not a change of direction. Decision: **no-go** on the acceptance as specified (see the last section).

## What was built

- `crates/parble-sim/src/chunk/field.rs`: a channel distance field over the block straights (`blk_straight_*`), 90° bends (`blk_curve90_*`) and one-cell slopes (`blk_slope_*`), chosen by piece id so no mechanism with the same port layout joins it. Landing floors (redirect, tower socket, catch), junctions, rails and mechanisms are not members and keep their current colliders and contacts.
- Each member's field is the distance from the ball centre to its exact channel wall: the `channel_tube_radius(r)` section (the radius the current exact contacts use, so the field meets the remaining exact surfaces and the facets at the same rest height) swept along its axis, the axis continuing straight (flat on a slope) past either port. The normal is the field's gradient (from the nearest wall point toward the centre), the penetration is radius minus distance.
- Combination: the minimum over the members' fields with each member's field taken as +∞ outside its own cell, so the member whose cell holds the point evaluates it. A plain minimum of tubes capped at their port planes is only a bound inside their union and would put a false wall at every seam. At a port plane two mating members agree in value and gradient (port rings and axis tangents match), so the owner switch adds no step.
- One contact per ball per step, from one field lookup at the end-of-step centre carried back along the normal (as every exact contact does), on the first field-facet manifold of the chunk that holds the owning member. Every other field-facet manifold, in that chunk or a neighbouring one, is cleared. No bounce while rolling (the existing `rolling` rule and the drop-approach guard). The CCD sweep skips the field facets (`ExactHold::Field`), as it does for the exact surfaces.
- Ownership: the field owns a ball while its centre is over a member's cell and each side neighbour within a ball diameter is a member or empty, the same guard `straight_owner` uses. Next to a junction, landing, rail or mechanism the current contact code runs unchanged.
- Determinism: new code uses `libm::sqrtf`/`floorf`, members live in a `BTreeMap` by cell, lookups run in a fixed order. The slope member reuses `ChannelSlope::contact` (four fixed Gauss-Newton steps), which calls `f32::sqrt`; that is IEEE correctly rounded on every target, and the wasm parity check below confirms it.
- Smooth minimum: not used. Junctions are out of scope, so there is no smooth-min displacement to report against `docs/kit-contract.md` tolerances.
- Section shape: the full circle, as the current exact contacts. The §5.1 vertical walls above the axis stay facets. For a shooter at `L3` the centre is only about 1.5 mm below the axis, so a full-circle field acts as a ceiling once it rises above the axis. Same as today's exact contacts. Not changed in the spike.
- Rendering is unchanged. Colliders are unchanged (the members are only recorded at install).

### The switch

`Sim::set_field_contacts(bool)` (and `TrackRun::set_field_contacts`), a per-world runtime flag, off by default. Why runtime and not a cargo feature: one binary runs both paths side by side, which every comparison below needs (per-seam before/after, both sweep tables, interleaved step timing under the same load, one wasm build exporting both hashes). A cargo feature is unified across the workspace, so enabling it for a test or for `parble-sim-wasm` would flip the default for every crate in that build. A per-world flag also never leaks between test threads, which a static would. With the flag off the hook's first check returns and no new branch runs. Every default golden is unchanged (`allpieces_30s` `88d798ff0c2a0ee4`, ramp, variants, mechanism, playroom, rope, lift).

## Criterion 1: one combined field, no facet hand-over between members

`chunk::field::tests::the_field_takes_every_owned_contact_on_straights_bends_and_slopes` counts, every tick, whether the centre's cell is field-owned and whether the field gave that tick's contact. Steep run (two `L3` straights, the steep slope, three `L1` straights): owned on 187 ticks, field contact missed on 0. Curve chain (`L3` straight, gentle slope to `L2`, straight, `blk_curve90_l2`, straight, the mirrored curve, gentle slope to `L1`, two `L1` straights): owned on 294 ticks, missed on 0. With the switch off the field gives no contact. On those owned ticks no other manifold reaches the contact hook with a solver contact (a test counter, counted before the hook edits anything, is 0 on both runs and asserted), so the field's one contact is the only surface the solver sees: no hand-over between member facets or exact surfaces. Ownership falls back to the current code only where a face neighbour within a ball diameter is a non-member piece.

## Criterion 2: `block_seam_test` seams on the field path

`block_seam_test::field_contacts_pass_the_seam_gates` (passes; asserts the field path only). Cat's eye, 0.3 m/s unless noted, mm/s. Limits are `limits(level)` unchanged. Gating follows the matching test: curves on kick, the z 90 mm seam logged, slope exits on upward with the kick logged (the steep exit's existing rule; the gentle exits' kick is over the limit on both paths because the floor's concave run-out sits in the window). The straight-to-straight and gentle-slope windows had no window before and are new.

| Seam | Current upward / kick | Field upward / kick | Limits | Gate |
|---|---|---|---|---|
| steep: straight_l3 to straight_l3 (x 30) | 0.00 / 0.00 | 0.00 / 0.00 | 2.19 / 5.19 | both |
| steep: straight_l3 to slope_steep_l3 (x 90) | 0.00 / 1.29 | 0.00 / 1.29 | 2.19 / 5.19 | both |
| steep: slope interior | 0.00 / 2.60 | 0.00 / 2.60 | 2.19 / 5.19 | kick |
| steep: slope to straight_l1 (x 150) | 0.00 / 24.35 | 0.00 / 24.35 | 2.19 / 5.19 | upward |
| steep: straight_l1 to straight_l1 (x 210) | 0.00 / 0.00 | 0.00 / 0.00 | 2.19 / 5.20 | both |
| steep: straight_l1 to straight_l1 (x 270) | 0.00 / 0.00 | 0.00 / 0.00 | 2.19 / 5.20 | both |
| steep run-out check | Ok (kick 24.35) | Ok (kick 24.35) | ceiling 60 | check |
| chain: straight_l3 to slope_gentle_l3 (x 30) | 0.00 / 0.77 | 0.00 / 0.77 | 2.19 / 5.19 | both |
| chain: slope_gentle_l3 to straight_l2 (x 90) | 0.00 / 5.23 | 0.00 / 5.23 | 2.19 / 5.19 | upward |
| straight_l2 to curve90_l2 | 73.65 / 3.27 | 73.65 / 3.27 | – / 5.19 | kick |
| curve90_l2 interior | 143.73 / 5.00 | 143.73 / 5.00 | – / 5.19 | kick |
| curve90_l2 to straight (z 30) | 3.08 / 3.91 | 3.08 / 3.91 | – / 5.19 | kick |
| straight to mirrored curve90_l2 (z 90) | 45.80 / 9.88 | 45.09 / **2.47** | – / 5.19 | logged |
| mirrored curve90_l2 interior | 93.36 / 3.23 | 94.23 / 3.27 | – / 5.19 | kick |
| mirrored curve90_l2 to slope_gentle_l2 | 0.07 / 1.93 | 0.04 / 1.80 | – / 5.19 | kick |
| chain: slope_gentle_l2 to straight_l1 (x 270) | 1.02 / 6.08 | 0.97 / 6.00 | 2.19 / 5.20 | upward |
| chain: straight_l1 to straight_l1 (x 330) | 0.76 / 0.03 | 0.73 / 0.03 | 2.19 / 5.20 | both |
| clean entry 0.4 m/s, z 90 seam | 51.53 / 1.12 | 51.53 / 1.12 | – / 5.19 | kick |
| clean entry 0.5 m/s, z 90 seam | 78.18 / 2.50 | 78.18 / 2.50 | – / 5.19 | kick |
| swing 0.035 m/s on straights | 2.03 / 0.09 | 2.03 / 0.09 | 2.19 / 5.19 | both |
| swing 0.12 m/s on straights | 22.67 / 1.09 | 22.67 / 1.09 | – / 5.19 | kick |

Every gated field value passes, with no threshold loosened. The field changes only the swinging-marble hand-over before the mirrored curve (kick 9.88 to 2.47, now under the limit). Everywhere else it reproduces the current numbers, because the current exact straight, bend and slope contacts are the same surfaces and the slope and bend contacts already reach a ball diameter past their cells along their straight continuations. Negative controls on the field path still fail the gates (`field_contacts_still_see_seam_bumps`): the steep entry port cap, upward 132.48; the 0.5 mm and 1 mm steps before the mirrored curve, kick 348.01 and 211.53.

## Criterion 3: all-pieces sweep (seeds 2-9 × 3 specs × 3 sizes, 72 runs)

| Path | Ok | Short | FellOff | Other | Mini Ok | Standard Ok | Shooter Ok |
|---|---|---|---|---|---|---|---|
| Current | 32 | 38 | 1 | 1 | 18 / 24 | 14 / 24 | 0 / 24 |
| Field | 33 | 32 | **5** | 2 | 20 / 24 | 13 / 24 | 0 / 24 |

**Not met**: 5 fall-offs against 1. Where they happen:

- 3 × steel standard (seeds 2, 3, 5), last placement 36 (`rail_joint_l1`): out of the tower landing too fast and off the rail. Seed 2 marble 6 crossed the joint at 0.357 m/s (the fastest current-path marble that seed crossed at 0.274 and stayed on). It entered the drop at 0.394 m/s, 1 mm off centre. On this track the field kept more plan speed through the `blk_curve90_l2` after the rail curve (steel standard mean, entry to the curve then to the straight after it: current 0.478 then 0.343, field 0.469 then 0.361), so the drop sees faster marbles. An energy trace of that marble (`entry_speeds` with `PARBLE_ENTRY_ENERGY=6:31:34`; ½v² + ⅕r²ω² + g·y per kg, ω from the tick-to-tick rotation) from the rail curve (31) through `blk_curve90_l2` (32) and the straight (33) shows no energy injected at the field's hand-overs: the field run has 2 tick-to-tick rises above 1 µJ/kg, summing 0.0009 J/kg, against 4 summing 0.0015 J/kg on the current path, 0.04 % and 0.06 % of the marble's 2.3 J/kg. What differs is loss, and the gap opens in the bend. Energy lost per placement, current then field: rail curve (31, not a member) 0.0171 and 0.0169 J/kg, `blk_curve90_l2` (32) 0.0981 and 0.0500 J/kg, straight (33) 0.0034 and 0.0027 J/kg. The marble enters the bend crossing the channel on both paths: sideways velocity 0.050 m/s (current) and 0.065 m/s (field) against 0.46 and 0.47 m/s along it, and on the current path its centre is 1.1 mm higher than the 0.2181 m it sits at on the following straights, so it is up the wall. This is the swinging entry where the seam table also shows the field smoothing the hand-over (z 90 mm kick 9.88 to 2.47). Clean centred bends show no difference (criterion 5 part below). Only seed 2 marble 6 was traced this way. Seeds 3 and 5 are attributed by their last placement (36) and spec alone. The tower landing and rail joint are not field members; this is their documented bounce sensitivity (see `tower_drop_stays_on_the_rails_for_glazed_and_steel`) meeting faster arrivals.
- 2 × shooter (seed 8 steel, seed 9 cat's eye), last placement 41: transfer 3. Traced on seed 9 only (seed 8 is attributed by its last placement): the shooter drops from the gate onto the `L3` straight 10 mm from its end-stop face, bounces 16 mm (0.66 m/s up, 0.30 m/s sideways) over the 2.5 mm the `L3` lip leaves a shooter, rolls on the redirect's top face and falls. That landing is within a ball diameter of the end stop, so the field does not own it: the contact code there is the current path's. The same runs on the current path ended Short with a shooter stuck at the end stop.

Diagnostic, not part of the criterion: seeds 10-17 (72 more runs) give the opposite sign. Current Ok 29 / Short 32 / FellOff 9 / Other 2 (mini Ok 21, standard Ok 8); field Ok 34 / Short 30 / FellOff 7 / Other 1 (mini Ok 20, standard Ok 14). Over all 144 runs: fall-offs 10 current, 12 field; Ok 61 current, 67 field. Both paths' fall-offs come from the transfer-3 shooter landing and the tower landing, which reshuffle on small arrival changes.

## Criterion 4: determinism and step cost

`make determinism-wasm` with the field path included (new `run_allpieces_field` export, `tests/golden/allpieces_30s_field.hash`, `tests/golden_allpieces_field.rs`): native `8a1f0c66a477ede5`, wasm `8a1f0c66a477ede5`, match. Every default golden matches too (the same run).

Step cost (`allpieces::tests::field_step_cost`, the sim side of `stress2000`: the all-pieces track and its 10 marbles, the tower being render-only; both paths interleaved in one release process, 5 rounds of 7,200 ticks):

| Round | Current µs per marble-step | Field µs per marble-step | Field / current | 1-min load before → after |
|---|---|---|---|---|
| 0 | 11.01 | 11.44 | 1.039 | 14.64 → 14.64 |
| 1 | 10.94 | 11.53 | 1.054 | 14.64 → 14.67 |
| 2 | 11.00 | 11.45 | 1.041 | 14.67 → 14.67 |
| 3 | 10.90 | 11.51 | 1.056 | 14.67 → 14.67 |
| 4 | 10.97 | 11.53 | 1.051 | 14.67 → 14.67 |
| Median | 10.97 | 11.51 | 1.049 | all 10 samples guarded (≤ 15) |

50,391 marble-steps per sample. The field costs about 5 % more per marble-step (0.54 µs), from the ownership neighbour checks and the end-of-step evaluation, which run before the current code's own checks rather than replacing them. The host had been at a 1-minute load of 20-200 in the hour before; the samples ran after it fell to 14.6 (`vm.loadavg`, which a 6-second run samples coarsely).

## Criterion 5, part: does the field recover the 7 % speed loss

Field contacts do not recover a systematic share of marble speed on the pieces they cover:

- On clean pieces the field and the current exact contacts are the same surfaces. Through one `blk_curve90_l2` (glazed mini, steel standard, cat's eye standard at 0.35 and 0.5 m/s) the field keeps exactly the current path's share (`bend_speed_retention`, ratio 1.000 in all six). Over six `L3` straights a glazed mini keeps 0.9410 on both paths against 0.9360 on bare facets (`straight_run_speed_retention`). The current exact contacts already took the facets' loss off these pieces.
- Seed 2 glazed mini, per-placement mean entry speed (`entry_speeds`; cups 3/2/3/2 on both paths): equal through the hopper and the first straights (straight 2: 0.224 / 0.224 m/s), mixed through the forks' curves (curve 17: 0.398 to 0.436; curve 19: 0.367 to 0.321), equal into the drop (34: 0.326 / 0.323). From the tower socket on the field run is faster: rail joint 0.166 to 0.182, checkpoint 0.136 to 0.153, one-way hole 0.129 to 0.146, one-way gate 0.122 to **0.140 m/s** (+15 %), straight after transfer 3 0.104 to 0.121. That gain starts at the tower landing, which is not a field member, with equal arrival speeds, so it is the landing's sensitivity, not a recovered loss. The glazed mini gate stall on the parked branch is not addressed by this spike.
- The one place the field does keep speed is a marble arriving off-centre and swinging: seed 2 steel standard enters the straight after `blk_curve90_l2` at 0.361 against 0.343 m/s (+5 %), and the traced marble loses 0.050 against 0.098 J/kg in the bend itself. That is a hand-over effect at one kind of entry, not the roughly 7 % loss, which the field does not recover.

## Go / no-go

**No-go** as a replacement for the current contacts.

- It works as designed: one field over straights, bends and slopes, no facet hand-over between members, every gated seam passes, wasm matches native, and the switch leaves the default path's goldens untouched.
- It brings almost no measured seam benefit, because the current exact straight, bend and slope contacts already give the same surfaces. The one change is the swinging hand-over before the mirrored curve (kick 9.88 to 2.47). It does not remove `CREASE_SINK_COMPENSATION_M` or the straight-owner rules: the field still needs `channel_tube_radius` to meet the facets and remaining exact surfaces at non-member seams, and an ownership guard at those seams.
- It costs about 5 % more native step time per marble (11.51 against 10.97 µs per marble-step, all samples load-guarded), because it adds checks ahead of the current code instead of replacing it.
- It fails criterion 3 on the specified sweep (5 fall-offs against 1). All 5 are at non-member pieces (tower landing and rail joint, the transfer-3 shooter landing) meeting slightly different arrivals.

What would change the call: making the tower landing and the transfer-3 `L3` landing robust to arrival speed on the current path first (they reshuffle fall-offs on either path, 10 and 12 over 144 runs). After that, rerun this sweep with the switch. The field's case then rests on simplifying the hook (one owner rule in place of per-surface claims), not on seam numbers. No follow-up cards are proposed.

> Code for this spike is kept unmerged on local branch `worktree-agent-a4e9d091984639512` (tip d872e23, base 4ade1ba). Only this note is on main.
