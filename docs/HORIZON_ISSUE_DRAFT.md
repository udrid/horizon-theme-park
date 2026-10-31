# Horizon reports: posted 2026-10-30

**Posted by udrid:** [#4966](https://github.com/horizon-rust/horizon/issues/4966) (UUID target finds only players) and
[#4968](https://github.com/horizon-rust/horizon/issues/4968) (`/ride` refuses non-mobs). Report 3: posted as a comment on [#3514](https://github.com/horizon-rust/horizon/issues/3514).

Below: the text as drafted, kept for reference.

All three differences were **checked in vanilla 26.3 by the owner** (Java Edition, single-player creative world,
cheats on, 2026-10-30) and in Horizon (installed Nightly 2026-10-30 `1da354b`, and a native build of `b5b451de`).

How to post (you do it, signed in as `udrid`):
1. Open <https://github.com/horizon-rust/horizon/issues/new>.
2. Report 1 and Report 2 each get their own new issue: paste the **Title** and the **Body** (everything under the line).
3. Report 3 is a **comment** on [#3514](https://github.com/horizon-rust/horizon/issues/3514) (their ledger already lists display
   entity loading as missing), not a new issue.
4. Don't add labels; the maintainers do that. Send me the issue numbers afterwards.

Before posting, check these two lines:
- The first line of each body says it was drafted by an AI agent. That follows Horizon's `AGENTS.md`. Edit it if you prefer.
- Nothing private is included (no paths, no tokens, no personal data).

---

## Report 1 (new issue)

**Title:** Commands: a UUID target (`/tp`, `/kill`, `/data get`) finds only players, not displays or other entities

**Body:**

> 🤖 Filed by udrid's AI agent (Claude), not by udrid personally.

In vanilla 26.3 a UUID given as a command target selects any entity. In Horizon it only selects players.

Reproduce in a fresh creative world, single-player, no mods:

```
/summon minecraft:item_display ~ ~2 ~ {UUID:[I;1,2,3,4],item:{id:"minecraft:diamond_block",count:1}}
/data get entity @e[type=minecraft:item_display,limit=1] UUID
/tp 00000001-0000-0002-0000-000300000004 ~ ~5 ~
/kill 00000001-0000-0002-0000-000300000004
```

- Vanilla 26.3: `/tp <uuid>` teleports the display (I tested `/tp` only; `/kill <uuid>` and `/data get entity <uuid>` should behave the same).
- Horizon: the second line prints `[I; 1, 2, 3, 4]` (the UUID is kept), but `/tp`, `/kill` and `/data get entity <uuid>` all
  answer "No entity was found". The same happens for an armor stand, a marker, an interaction and a pig summoned with an
  explicit `UUID`. The player's own UUID works. `@e[nbt={UUID:[I;1,2,3,4]},limit=1]` finds the entity.

What I read in the code (commit `b5b451de`): `target_by_uuid` and `target_with_uuid`
(`game/commands/players.rs`, `game/commands/execute.rs:771`) check the player, then `persist.uuids`, then `self.mobs`.
A display is a `Target::Other` held in `self.others`. I don't know whether `persist.uuids` is filled for entities summoned
with a `UUID` tag, so I can't say which of those is the cause.

Why it matters to a mod: selector commands scan every entity, so moving N displays with `@e[tag=x,limit=1]` costs about
N × (entities in the world). With 1,000 displays, 400 `tp @e[tag=x,limit=1]` per tick took a tick to about 280 ms. A UUID lookup
would make each move cheap.

---

## Report 2 (new issue)

**Title:** `/ride` refuses every vehicle that is not a mob (display entities, armor stands, minecarts...)

**Body:**

> 🤖 Filed by udrid's AI agent (Claude), not by udrid personally.

In vanilla 26.3 `/ride <player> mount <entity>` mounts a display entity. In Horizon it fails.

Reproduce in a fresh creative world, single-player, no mods:

```
/summon minecraft:item_display ~ ~2 ~ {item:{id:"minecraft:diamond_block",count:1}}
/ride @s mount @e[type=minecraft:item_display,limit=1]
```

- Vanilla 26.3: "started riding"; you sit on it and Shift gets you off.
- Horizon: `<player> couldn't start riding Item Display`.

The same refusal happens for block_display, text_display, armor_stand, interaction, marker, minecart and
area_effect_cloud. A pig, a chicken and every other mob I tried (17 types) works. The cause is in `cmd_ride`
(`game/commands/world_cmds.rs`): anything that is not a `Target::Mob` goes to `commands.ride.mount.failure.generic`.

I did not check vanilla for the non-display types (armor stand, minecart, etc.), only for the item display, so please treat
those as "probably the same".

---

## Report 3 (comment on #3514)

> 🤖 Agent (udrid's, Claude): related to `Display.readAdditionalSaveData` being listed as missing.

Evidence that this also affects `/data merge` and `/data modify` on a **live** display, not only on saved ones.
Vanilla 26.3 turns the display; Horizon reports "Nothing changed":

```
/summon minecraft:item_display ~ ~2 ~ {item:{id:"minecraft:diamond_block",count:1},transformation:{left_rotation:[0f,0f,0f,1f],right_rotation:[0f,0f,0f,1f],translation:[0f,0f,0f],scale:[1f,1f,1f]}}
/data merge entity @e[type=minecraft:item_display,limit=1] {transformation:{left_rotation:[0f,0.38268f,0f,0.92388f],right_rotation:[0f,0f,0f,1f],translation:[0f,0f,0f],scale:[1f,1f,1f]}}
```

- Vanilla 26.3: "Modified entity data of Item Display"; the block turns 45°.
- Horizon (Nightly `1da354b` and `b5b451de`): `Nothing changed. The specified properties already have these values`.

`/data modify entity … transformation.left_rotation set value […]` gives the same message, as do merging `item` or
`view_range`; `interpolation_duration` can't be read back ("Found no elements matching"). `/tp <target> x y z <yaw> <pitch>`
does rotate the display, so only the NBT editing is missing.


---

## Optional: follow-up comment for #4966 (now we know the cause)

> 🤖 Agent (udrid's, Claude): the cause, from reading `b5b451de`.

`target_with_uuid` (`game/commands/execute.rs:771`) does find the entity: a summoned display's UUID is stored in
`persist.uuids` (`persist.rs:817`, `remember(id, uuid, tag)` from `load_other`). It then ends with
`self.mobs.iter().any(|m| m.id == id).then_some(Target::Mob(id))`, and a display is a `Target::Other` held in
`self.others`, not in `self.mobs`, so the result is None. Checking `self.others` too (and the other entity lists:
armor stands, boats, minecarts...) should make `/tp`, `/kill` and `/data get` work by UUID for those entities.
I haven't tried the change.
