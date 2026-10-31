# M2: proposed ARCHITECTURE §4 wording (DRAFT for the owner's approval)

Nothing in `docs/ARCHITECTURE.md` has been changed. If you approve, a builder replaces the
two places below with this wording.

## 1. The substep recipe (replaces the block under "Each substep, for each train")

The order of the limits changed: the elements' limits are applied to the speed BEFORE the position
moves (approved in the M2 review). Elements never jump the speed.

```
for each car i:  f_i = table.frame_at(s_i)
                 K_i = (forward(s_i + h) − forward(s_i − h)) / (2h)      h = 0.25 block   [D15]
                 A_i = a_prev · f_i.forward + v² · K_i                    (rider acceleration)
                 F_i = A_i − (0, −G, 0)                                    (push the seat gives)
                 N_i = |F_i − (F_i · f_i.forward) f_i.forward|             (normal push, blocks/s²)

a_gravity  = −G · mean_i(f_i.forward.y)                                    [D13]
a_friction = −sign(v) · friction · mean_i(N_i)                             [D14]
a_drag     = −sign(v) · drag · v²                                          [D14]
a          = a_gravity + a_friction + a_drag
a          = element changes to a (below)
v = v + a·dt                    ← speed first
apply element limits to v (lift/push/wheels pull, brake floor, no rolling back on the lift)
s = s + v·dt                    ← then position with the NEW speed (semi-implicit Euler)   [D12]
wrap s; if the front car reaches the station's stop point, snap there and hold
```

- `a_prev` is the previous substep's actual speed-up `(v_after − v_before)/dt`, element effects included.
- At very low speed (`|v| < 0.05` with `|a_gravity|` under the friction limit), set `v = 0`, except on the lift and in the station.
- The felt-g text and the HUD rules stay as they are. Level frames (D59): on unbanked track "up" is the sky projected square to the track.

## 2. The element table (replaces "Elements (behaviour fixed, default numbers OPEN …)")

"On an element" means any car of the train is on it [D13]. Numbers are the approved ones [D57].
Pulling elements accelerate at `ELEMENT_ACCEL = 1.5 blocks/s²` (about 0.15 g) and never jump the speed.

| Element | Behaviour | Default |
|---|---|---|
| Station | The front car follows a slowing curve `v = √(2·1.0·distance)` (between 0.05 and 3 blocks/s; slower trains are pulled along) and stops exactly at the stop point. It holds there. When someone sits, a 5 s countdown starts (cancelled if everyone leaves); then it pushes out up to the push speed. A station stretch must not cross the lap's start (`Course::validate`). | dwell 5 s, push 2 blocks/s |
| Lift { speed } | If `v < speed`: pull up to `speed` at `ELEMENT_ACCEL`, first cancelling gravity and friction. Holds `speed` against gravity and never lets the train roll back (`v ≥ 0`). A train already faster just coasts. | 5 blocks/s (18 km/h) |
| Brake { target_v } | If `v > target_v`: `a −= brake_decel` until `v ≤ target_v`. A brake only removes speed, so on real track the train may keep slowing below the target. Brakes do not stack with the station's curve. | decel 6 blocks/s², target 3 blocks/s |
| BlockBrake | (M4) If the next section is taken: stop and hold (`Held`). Else act as a trim brake. | pass 3 blocks/s |
| Booster { force } | (M4) `a += force` | 8 blocks/s², no speed cap (OPEN) |

## 3. Required tests (add to the list under "Required tests")

8. Lift: never rolls back, pulls up gently, leaves at the chain speed.
9. Brake (frictionless track): leaves at `target_v` ± 0.1.
10. Station: stops within 0.2 of the stop point, holds for the dwell, pushes out gently.
11. Demo lap: never stalls (except in the station) and ends in the station; nothing jolts
    (acceleration at most 15 blocks/s², forward/back push at most 1 g).
12. Demo limits (D58): drops 50–65°; valleys at most 4 g; airtime between 0 and −0.5 g; sideways at most 1 g;
    vertical g changes by at most 3 g over any 1-second window; top speed at most 30 blocks/s; lift 20–30 blocks.
13. Back row over the lift crest is clearly faster than the front row was (D13).
