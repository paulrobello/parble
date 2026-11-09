# Coefficient of restitution: glass marble on hardwood

Researched 2026-11-09 for parble `docs/marbles.md` and `crates/parble-sim/src/marble.rs`. Read-only on the repo.

## Bottom line

- **No source found measures a 10–25 mm glass sphere on maple, beech or birch.** Every number below is either a neighbouring system (another ball on wood, glass on another solid) or an inference. A sourced 0.60 does not exist in what I could retrieve, and I did not bend the evidence toward it.
- **Recommended glass on wood, face grain: 0.70**, plausible range 0.60–0.80. Confidence low to medium.
- **Recommended glass on wood, end grain: 0.75**, plausible range 0.65–0.85. Confidence low. This is an inference from hardness, not a measurement.
- **Is 0.66 too high? Not demonstrated.** It sits inside the plausible range, slightly below the centre. The claim in `docs/marbles.md` that 0.66 is an "upper bound" is not supported: glass on very hard surfaces measures about 0.97, so a 0.658 result on concrete is more likely a sloppy student measurement than a ceiling.
- If 0.60 is chosen, justify it as "thin or damped wooden track piece" (see the plate-thickness finding), not as "glass on hard wood".
- The cheapest way to get a defensible number is to measure it: drop a marble on the actual kit wood and read the bounce times from audio (protocol at the end).

## Sources

### S1. Sakai, Murata, Nakamura 2022, ball on eight hardwoods (closest wood data)

- Citation: "Comparison of Collision Behaviors of Several Wood-Species Using Image Analysis", J. Soc. Mater. Sci. Japan 71(7), 603–607. https://www.jstage.jst.go.jp/article/jsms/71/7/71_603/_article/-char/en (full text PDF is on the same page, Japanese body, English abstract and figure labels).
- Conditions: free-fall, vacuum-released, no spin. Ball: **hard plastic park-golf ball, 60 mm, 830 kg/m3, not glass**. Targets: 200 x 80 x 40 mm blocks at 7.4–10.7 % moisture, **edge grain (quartersawn) or comb grain**, 24 impacts each, 600 fps video. Drop height is not stated in the text I read.
- Values read from Fig. 9 (read off the plot, approximate): alder 496 kg/m3 about 0.70, magnolia 512 about 0.68, birch (*Betula platyphylla*) 557 about 0.71, oak (*Quercus crispula*) 605 about 0.755, maple (*Acer pictum*) 748 about 0.78, ash (*Fraxinus mandshurica*) 805 about 0.76, eucalypt 876 about 0.78.
- Finding: COR rises linearly with density from 400 to 600 kg/m3 and **saturates above 600 kg/m3**. Woods below 600 kg/m3 dent permanently, woods above show no dent (Table 4). Repeated hits on one spot raised COR slightly.
- Applicability: good for the density and denting trend and for the plateau (maple, beech, dense birch all sit in it). The ball is plastic, 60 mm and much softer than glass, so the absolute value does not transfer. A stiffer, smaller glass marble concentrates load and should dent the wood earlier, which pushes COR down relative to this plastic ball.

### S2. Haron and Ismail 2012, sports balls on wood versus steel

- Citation: "Coefficient of restitution of sports balls: A normal drop test", IOP Conf. Ser.: Mater. Sci. Eng. 36, 012038. doi:10.1088/1757-899X/36/1/012038. https://iopscience.iop.org/article/10.1088/1757-899X/36/1/012038/pdf
- Conditions: drop heights 1.3–1.7 m (about 5 m/s), 25 mm thick plate on a steel table, wood species not stated.
- Values (steel / wood): golf ball 0.897 / 0.681, table tennis 0.809 / 0.786, hockey 0.647 / 0.398, cricket 0.615 / 0.356 at 1.3 m. Reductions on wood: 4 %, 24 %, 40 %, 43 %. The authors tie the loss to ball mass. COR falls slightly with drop height on both targets.
- Applicability: shows that wood costs a hard ball roughly 5–25 % of its COR at 5 m/s, scaling with ball mass. A 5 g marble (16 mm) is between the table tennis and golf balls in mass. Our impacts are 5–50 times slower, which should reduce the loss. Species and plate support are uncontrolled.

### S3. Sondergaard, Chaney, Brennen 1990, spheres on plates

- Citation: "Measurements of Solid Spheres Bouncing Off Flat Plates", J. Appl. Mech. 57(3), 694–699. https://resolver.caltech.edu/CaltechAUTHORS:SONjam90 (PDF: https://authors.library.caltech.edu/records/prznw-f4w71/files/SON100.pdf)
- Conditions: steel, bronze and Pyrex glass spheres 3.2–25.4 mm, lucite and aluminium plates 3.2–38 mm thick, impact speeds 1.7–3.8 m/s for the velocity series.
- Findings relevant to us:
  - **Plate thickness**: e depends almost only on the ratio of sphere diameter to plate thickness. For Pyrex on lucite (Fig. 4) e is about 0.8–0.9 for ratios below 0.5, falls to about 0.5–0.6 near ratio 1, and to 0.2 or less near ratio 2.5. The loss is energy carried off by the plate (Zener 1941), which a later collision can recover, so it is not necessarily dissipation.
  - **Distance from a support**: e drops with distance from a clamped edge until a critical distance of about 2.75 sphere diameters.
  - **Velocity**: weak at the low end of their range, trending toward v^(-1/4) at high speed. Citing Johnson (1985), e is essentially 1 until the yield velocity, which is very small for most materials.
- Applicability: not wood, but the thickness effect is the most relevant result for toy runs. A 16 mm marble on a 6 mm track plate (ratio 2.7) behaves very differently from the same marble on a thick block. If parble track pieces are modelled as thin wood, a lower effective COR is justified by this paper.

### S4. Joseph, Zenit, Hunt, Rosenwinkel 2001, glass and steel on a hard wall (upper reference)

- Citation: "Particle-wall collisions in a viscous fluid", J. Fluid Mech. 433, 329–346. https://scispace.com/pdf/particle-wall-collisions-in-a-viscous-fluid-363q701gtw.pdf
- Value as summarized by a search result, not read in full by me: mean particle-wall COR about 0.97 +/- 0.02 for glass and steel spheres on a glass-ceramic wall in air at 40–360 mm/s, consistent with Foerster et al. 1994. Thin glass walls gave lower values.
- Applicability: sets the hard-surface ceiling at about 0.97 in our speed range. The same speed band as parble impacts (0.04–0.4 m/s).

### S5. Foerster, Louge, Chang, Allia 1994, collision properties of small spheres

- Citation: Phys. Fluids 6(3), 1108–1115. doi:10.1063/1.868282. Metadata confirmed by search, full text not read.
- Covers polystyrene, acrylic and stainless steel spheres on flat walls. No glass and no wood. Useful only as the method reference behind S4.

### S6. Montaine et al. 2011 and the nonmonotonic-velocity paper

- Montaine, Heckel, Kruelle, Schwager, Pöschel, "The Coefficient of Restitution as a Fluctuating Quantity", arXiv:1104.0049, 3 mm stainless steel ball on a hard plate at 0.3–1.4 m/s. Shows large shot-to-shot scatter of e at a fixed speed, so any single drop is noisy.
- "Inelastic impact of a sphere on a massive plane: nonmonotonic velocity-dependence of the restitution coefficient", arXiv:cond-mat/0209490. e depends non-monotonically on speed because of plate vibration modes, metal spheres on steel, granite and similar. No wood.
- Applicability: velocity dependence at 0.1–1 m/s is weak to non-monotonic for hard targets. Expect a measured wood value to be flat within the scatter. Both papers use hard targets, so they bound the behaviour rather than give wood numbers.

### S7. The Physics Factbook, "Coefficients of Restitution" (current citation)

- https://hypertextbook.com/facts/2006/restitution.shtml
- A 2005 high-school experiment. Balls dropped from 92 cm, COR = sqrt(bounce/drop), five trials: glass marble 0.658, wooden ball 0.603, steel ball bearing 0.597, billiard ball 0.804.
- **Surface: concrete, as read on the page.** Not wood. No marble diameter, no drop-to-bounce measurement method beyond eyeballing. It also reports a steel bearing at 0.597, which is far below the 0.9+ that steel on a hard slab normally gives, so the concrete or the method is damping the result. Treat as weak.
- Our `docs/marbles.md` says the values were "measured on a hard concrete floor", which matches this page. The "upper bound for a marble on a wooden block" sentence is an inference, not something this source supports.

### S8. Other leads, not retrieved or not applicable

- Sadoh, Nakajima, Ohira 1983, "Resilience of balls impacting on wood surfaces", Mokuzai Gakkaishi 29(1), 8–13. Cited by S1 for wood-surface ball rebound. Bibliographic record only (https://agris.fao.org/search/ru/records/64774c5f5eb437ddff734df8). The best lead for real grain-direction data, if someone can get the paper.
- Sadoh and Nakato 1987, Wood Sci. Technol. 21, 111–120, doi:10.1007/BF00376191. Abstract says rebound resilience and hardness are governed by different mechanisms. Not read.
- Secondary tables list steel on wood at about 0.50 (Stanford Advanced Materials, https://samaterials.com/content/coefficient-of-restitution.html). Unsourced, species unknown. Weak.
- Lab handouts (Coefficients of Restitution, kvis.ac.th) suggest comparing steel balls on different grain directions but report no results.
- A resin/glass/steel drop test onto acrylic and steel plates reported about 0.83, 0.68 and 0.55. I could not identify the paper, so it is not cited as evidence.

## Dependence summary

| Factor | What the evidence says | Source |
|---|---|---|
| Impact speed, 0.1–1 m/s | Weak. e is near 1 below the yield speed, then falls slowly. No wood data in this range. | S3, S6 |
| Wood density | COR rises to about 600 kg/m3 then plateaus. Maple, beech, birch near or above the plateau. | S1 |
| Wood denting | Only woods below about 600 kg/m3 dent from a 60 mm plastic ball. A glass marble may dent denser woods. | S1 |
| Grain, face versus end | No COR data found. Janka end hardness is generally higher than side hardness (a static test). Inference: end grain is at least as bouncy as face grain. Edge grain only was tested in S1. | S1 (edge grain only), Janka |
| Plate or block thickness | Strong when diameter/thickness exceeds about 0.5. Blocks 40 mm or thicker are safe. | S3 |
| Ball mass | Heavier balls lose more on wood. | S2 |

## Comparable data

- **Glass on glass or hard glass-ceramic**: about 0.97 at 40–360 mm/s (S4). Lower if the target is a thin plate (S3).
- **Steel on wood**: no direct measurement found. The Factbook 0.597 is for concrete and suspect (S7), the 0.50 steel/wood table entry is unsourced. S2 suggests steel, being heavier than glass, would lose more on wood. Keeping steel at or below glass is consistent, which the current table already does.
- **Plastic ball on hardwoods**: 0.68–0.78 (S1), as a soft-ball reference point.

## Recommendation

- **Glass on face-grain hardwood: 0.70.** Basis: the hardwood plateau in S1 (about 0.76 for a plastic ball), nudged down because glass is stiffer and may dent the surface, and because S2 shows wood always costs a hard ball something. Range 0.60–0.80.
- **Glass on end grain: 0.75.** Basis: higher end hardness and no tested contradiction. Range 0.65–0.85. No data, so label it an assumption. If the sim cannot tell end grain from face grain at a contact, one value of 0.70 is defensible.
- **0.60** is the low end of the plausible range. It is reasonable for thin or hollow track pieces and for a tuned "feel", where S3 supports a lower effective COR. It is not supported as a property of glass on hard wood.
- **0.66** is inside the range. Not demonstrably too high. Update the "upper bound" wording in `docs/marbles.md`: 0.658 is a concrete-floor classroom value of low quality, and glass on hard surfaces measures higher.

## How to get a real number cheaply

Drop each marble (10, 16, 25 mm, glass and steel) from 5, 20 and 60 mm onto a flat piece of the kit wood, resting on a hard bench, face grain and end grain, 10 drops each. Record audio on a phone and take the times between successive impacts. For a bounce sequence, e = t(n+1)/t(n), which needs no height measurement and works at the speeds of interest (Montaine et al. describe the same acoustic method). Report the mean and spread per condition. That gives sourced, in-house values, covers the speed dependence, and resolves the 0.60 versus 0.66 question directly.

## Gaps

- No direct glass-on-wood paper found. Searches covered restitution plus wood, hardwood, maple, beech, birch, oak, end grain, glass marble, steel ball, DEM calibration tables, and croquet.
- Sakai values are read from a plot, not tabulated. The drop height is not given.
- S4's 0.97 comes from a search summary of the paper, not from my reading of the full text.
- Sadoh 1983 and Sadoh and Nakato 1987 could not be retrieved.
