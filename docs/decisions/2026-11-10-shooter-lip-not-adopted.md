# Shooter containment lip (kit contract 2): not adopted

Date: 2026-11-10. Decision: Paul, check-in answer to Q22 ("use recommended").

## Question

Card 01a1c3c1 asked for shooter (25 mm) marbles to stay inside block channels on landing bounces. Kit contract 2 proposed a lip rim on every L3 block:
- shooter containment depth 5.5 mm;
- lip height 3 mm and width 4 mm;
- a 5 mm lip band;
- `blk_end_l3`'s rim raised to 65 mm over its cap.

The contract change was enforced by kit-check §14.11.

## Result

On the 72-run all-pieces sweep (seeds 2 to 9), the lip removed every fall-off but cut completions from 32 to 22. A narrower fix was also offered: the exact straight-tube contact skips a marble centred above the tube axis (the "lid").

## Decision

- Kit contract 2 is not adopted. `docs/kit-contract.md` stays at contract 1, and the current all-pieces outcomes stand: Paul accepted them on 2026-11-09.
- The lid fix is not taken on its own.
- The lip is revisited after the landing-robustness work (card 01a1c4c0) lands, since the tower-socket and transfer-3 landings are the common root of the fall-offs.

The code stays unmerged on branch `worktree-agent-a3bafd93084c9599b`:
- 14bc976 and 046e78a: the lip, the contract and the kit rebuild;
- 4903d61: the lid fix;
- a4d811e: the contact-trace diagnostics.
