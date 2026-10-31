# Handoff: from the engine-spike session to the M0 planning session

Written 2026-10-30 by the Sonnet 5.5 session that ran the engine spike. The next session is **Claude Opus 5.5**
(CLAUDE.md §5: Opus plans each milestone). Read this file first, then the files it points to.

## How to start the next session (owner)

1. In the Claude desktop app: **New session**, folder `C:\Users\Shadow\Desktop\Horizon minecraft\horizon-theme-park`.
2. Pick the model **Claude Opus 5.5** in the model picker before sending the first message.
3. Paste the block below as the first message.

```
Read docs/WORKING_WITH_ME.md first and follow it (it overrides everything else), then CLAUDE.md and docs/HANDOFF_M0.md.
I'm a beginner and want to learn as we go.

You are the Opus planner for milestone M0. A Sonnet session already wrote a DRAFT plan in docs/milestones/M0.md, but
CLAUDE.md says Opus plans milestones, so please review it critically and rewrite it as the proper plan. Don't re-argue
recorded decisions (D1–D32); if you think one is wrong, say why and ask me. Don't build anything, don't download anything
and don't edit docs/ARCHITECTURE.md or docs/DECISIONS.md without asking. Bring me the open choices (C1–C4 in the draft,
plus any you find) as 2–3 options each with a recommendation, and wait for my answers. When I've approved, record them
in docs/DECISIONS.md. Teach me the new terms and add them to docs/LEARNING.md.
```

## Where the project stands

**Done (all in `docs/`):**
- The **engine spike** ran. Results, numbers and the reading follow-up are in `SPIKE_RESULTS.md`.
- **Decisions D23–D32** are recorded (mob seats; `tp` yaw/pitch rotation; cap on world entities; cars `teleport_duration` and
  aim-ahead; display budget 1,500; fling-off needs a private call; seats moved by rider name; adaptive rail pieces; group parking).
  `ARCHITECTURE.md` §5 command table, §6 rendering bullets and risk R1 were updated to match.
- `COASTER_OPTIONS.md` is the research on cheaper ways to move and draw things (and what failed).
- Three reports were posted to Horizon by the owner: issues #4966 (UUID targets) and #4968 (`/ride`), and a comment on #3514
  (`data merge` on displays). Draft text, plus an unposted follow-up comment for #4966 with the exact cause, is in
  `HORIZON_ISSUE_DRAFT.md`.
- `milestones/M0.md` is a **draft by Sonnet** (the mistake the owner caught: it should have come from Opus).

**Not done / open:**
- **Owner's answers to C1–C4** in the M0 draft (mod id, how `park-bundle` reads Rust, what the "loaded" message does, committing docs).
- **Nothing is committed.** Changed or new files since the last commit: `ARCHITECTURE.md`, `DECISIONS.md`, `LEARNING.md`,
  `SPIKE_RESULTS.md`, `COASTER_OPTIONS.md`, `HORIZON_ISSUE_DRAFT.md`, `AGY_QUESTIONS.md`, `milestones/M0.md`, this file, and
  the untracked `spike/`. **Do not commit `spike/`** unless the owner says so. **Never push.**
- ARCHITECTURE.md §5's `Host` trait code block and a few older sentences still show the pre-spike design (separate
  `set_rotation`, seats as displays). A docs-sync task (T6 in the M0 draft) is planned; show the owner the wording first.
- The **fling-off** (D22/D28) and the private **move-by-UUID** call are designed on paper only (SPIKE_RESULTS.md, "Reading
  follow-up"). Nothing private is approved or built.
- The **Horizon dedicated-server / Bloom hosting question** was not rechecked.

## What the spike taught (the facts that shape M0)

- Hook API 21 is at Horizon commit `b5b451de7c9d8d3dfc851c9405d4754724205e1b`. The launcher's `native-build` builds exactly that
  commit for a mod that pins it. The owner's installed game is older (API 7), which does not matter for the mod.
- A `[[module]]` entry in `horizon-mixin.toml` adds one `.rs` file to the game as `crate::game::<name>`; a `HEAD` injection on
  `native_server_tick` or `native_world_lifecycle` calls into it. Proven by `spike/native-spike/`.
- `native_run_command` works from the tick hook in single-player without `/publish`. It returns only success or failure.
- `time query daytime` is not a valid command in 26.3; use `time query gametime`.

## Practical notes for the next agent

- **Building:** the owner runs `horizon-launcher native-build <folder> --trust-native-code` themselves. First build was 836 s;
  rebuilds about 6 min; no further downloads expected (Rust and crates are cached).
- **Testing without the owner:** run the game in a scratch home (`HORIZON_HOME` in the session scratchpad,
  `horizon-ctl --socket <file> launch --world <name> --creative --test-terrain --wait`), then read `logs/latest.log` for
  `htp_spike` / mod lines. `spike/c2.sh` is a helper, but its socket path points into a **previous session's** scratchpad, so edit it.
  Chat can cover the screen; `options-set chatVisibility 2` hides it for screenshots.
- **The free reader (`agy`)**: its weekly allowance ran out on 2026-10-30 and resets in about 163 hours. A plain read-only run is
  `agy --mode plan -p "<prompt>"` from a folder containing an exported copy of the code (no permission prompts). A long prompt
  with the default model timed out at 15 minutes; use smaller batches. Ask the owner before using it.
- **Desktop control:** the Minecraft window can't be granted to computer use (not in the installed-apps list), so the owner types
  commands in their own game when a vanilla test is needed.
- **Rust for our own tests:** official rustup, `gnu` target; add it to PATH per command (see CLAUDE.md §3).
- **The "Native mods" launcher instance** currently holds the spike build (`native-b5b451d-…`). A new `native-build` of the M0 mod
  replaces it.
- **Mistakes to avoid** (the owner caught or I made them): writing a milestone plan as Sonnet; stating what vanilla does without the
  owner testing it; running a free-reader job without warning it uses the owner's allowance.

## Files to read, in order

1. `docs/WORKING_WITH_ME.md` and `CLAUDE.md` (rules)
2. `docs/milestones/M0.md` (the draft to review)
3. `docs/DECISIONS.md` (D1–D32) and `docs/ARCHITECTURE.md`
4. `docs/SPIKE_RESULTS.md` (evidence) and `docs/COASTER_OPTIONS.md` (research)
5. `spike/native-spike/mixins/htp_spike/` (a working example of a native mod: `horizon-mixin.toml`, `tick.rs`, `spike.rs`)
