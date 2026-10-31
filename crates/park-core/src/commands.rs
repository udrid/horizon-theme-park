//! The exact text of every command the mod sends to the game (D43).
//!
//! Each function builds one command as a `String`. Nothing here talks to the game, so a test can
//! compare the text word for word. The runtime decides *what* to send; the glue just runs it.
//!
//! Numbers are written with 4 decimals and a `.` point, never with an exponent (`1e-5`),
//! and `-0` is written as `0.0000`, because the game's command reader wants plain numbers.

use glam::{DQuat, DVec3};

/// The ride id of the demo coaster.
pub const DEMO: &str = "demo";

/// The model items of the content mod (D47).
pub const RAIL_ITEM: &str = "htp:rail";
pub const CAR_ITEM: &str = "htp:car";

/// What a seat is made of (D49). To switch the seats to saddled invisible pigs, change this one
/// function and `summon_seat` below (see `summon_seat` for the pig version).
pub const SEAT_ENTITY: &str = "minecraft:minecart";

/// A number as the game wants it: 4 decimals, no exponent, and never "-0.0000".
pub fn num(v: f64) -> String {
    let text = format!("{v:.4}");
    if text == "-0.0000" {
        "0.0000".to_string()
    } else {
        text
    }
}

/// A number with the `f` (float) ending that NBT data wants.
fn float(v: f64) -> String {
    format!("{}f", num(v))
}

/// Three numbers in a row, for a position.
fn xyz(p: DVec3) -> String {
    format!("{} {} {}", num(p.x), num(p.y), num(p.z))
}

/// The tag every entity of the mod carries.
pub const TAG_ALL: &str = "htp";

/// The tag of everything that belongs to one ride, e.g. `htp_demo`.
pub fn tag_ride(ride: &str) -> String {
    format!("htp_{ride}")
}

/// The tag of one car, e.g. `htp_car_demo_0`.
pub fn tag_car(ride: &str, index: usize) -> String {
    format!("htp_car_{ride}_{index}")
}

/// The tag of one seat, e.g. `htp_seat_demo_1`.
pub fn tag_seat(ride: &str, index: usize) -> String {
    format!("htp_seat_{ride}_{index}")
}

/// `Tags:["htp","htp_demo","…"]` with one extra tag for the specific piece.
fn tags(ride: &str, extra: Option<String>) -> String {
    let mut list = format!("\"{TAG_ALL}\",\"{}\"", tag_ride(ride));
    if let Some(extra) = extra {
        list.push_str(&format!(",\"{extra}\""));
    }
    format!("Tags:[{list}]")
}

/// The `transformation` data of a display: rotation, no shift, a size.
fn transformation(rotation: DQuat, scale: DVec3) -> String {
    format!(
        "transformation:{{left_rotation:[{},{},{},{}],right_rotation:[0.0000f,0.0000f,0.0000f,1.0000f],translation:[0.0000f,0.0000f,0.0000f],scale:[{},{},{}]}}",
        float(rotation.x),
        float(rotation.y),
        float(rotation.z),
        float(rotation.w),
        float(scale.x),
        float(scale.y),
        float(scale.z)
    )
}

/// Put one piece of rail on the track. `rotation` turns the model so its long side follows the
/// track, `scale` is (width, height, length) in blocks. Rails never move, so no `teleport_duration`.
pub fn summon_rail(ride: &str, pos: DVec3, rotation: DQuat, scale: DVec3) -> String {
    format!(
        "summon minecraft:item_display {} {{{},item:{{id:\"{RAIL_ITEM}\",count:1}},{}}}",
        xyz(pos),
        tags(ride, None),
        transformation(rotation, scale)
    )
}

/// Put a car on the track. `teleport_ticks` is how long the display glides to each new spot
/// (it can't be changed later, D44); `scale` is the size of the car's shell model.
pub fn summon_car(
    ride: &str,
    index: usize,
    pos: DVec3,
    yaw: f64,
    pitch: f64,
    teleport_ticks: u32,
    scale: DVec3,
) -> String {
    format!(
        "summon minecraft:item_display {} {{{},Rotation:[{},{}],item:{{id:\"{CAR_ITEM}\",count:1}},teleport_duration:{teleport_ticks},{}}}",
        xyz(pos),
        tags(ride, Some(tag_car(ride, index))),
        float(yaw),
        float(pitch),
        transformation(DQuat::IDENTITY, scale)
    )
}

/// Send a car to a new spot, turned to face `yaw`/`pitch` (Minecraft's angles, in degrees).
pub fn move_car(ride: &str, index: usize, pos: DVec3, yaw: f64, pitch: f64) -> String {
    format!(
        "tp @e[tag={},limit=1] {} {} {}",
        tag_car(ride, index),
        xyz(pos),
        num(yaw),
        num(pitch)
    )
}

/// Put one seat on the track: a minecart the player boards by right-clicking it (D48, D49).
///
/// Returns a list of commands because another seat kind may need more than one. For a saddled
/// invisible pig (the fallback in D49) the list would be:
/// `summon minecraft:pig … {…,equipment:{saddle:{id:"minecraft:saddle",count:1}}}` followed by
/// `effect give @e[tag=<seat tag>] minecraft:invisibility infinite 0 true`.
pub fn summon_seat(ride: &str, index: usize, pos: DVec3) -> Vec<String> {
    vec![format!(
        "summon {SEAT_ENTITY} {} {{{}}}",
        xyz(pos),
        tags(ride, Some(tag_seat(ride, index)))
    )]
}

/// Move one seat (and whoever sits in it), found by its tag [D49].
pub fn move_seat(ride: &str, index: usize, pos: DVec3, yaw: f64, pitch: f64) -> String {
    format!(
        "tp @e[tag={},limit=1] {} {} {}",
        tag_seat(ride, index),
        xyz(pos),
        num(yaw),
        num(pitch)
    )
}

/// Show `text` on one player's action bar (the line above the hotbar). Used when the game door
/// has no direct way to do it. `text` must be plain text without quotes or backslashes.
pub fn action_bar(player_name: &str, text: &str) -> String {
    format!("title {player_name} actionbar {{\"text\":\"{text}\"}}")
}

/// The block the station platform is made of (D60).
pub const PLATFORM_BLOCK: &str = "minecraft:smooth_stone";

/// Fill the box between two corners (both included) with `block`. Corners are whole block numbers.
pub fn fill(from: [i32; 3], to: [i32; 3], block: &str) -> String {
    format!(
        "fill {} {} {} {} {} {} {block}",
        from[0], from[1], from[2], to[0], to[1], to[2]
    )
}

/// Remove everything that carries `tag`.
pub fn kill_tag(tag: &str) -> String {
    format!("kill @e[tag={tag}]")
}

/// Remove every entity of one ride.
pub fn kill_ride(ride: &str) -> String {
    kill_tag(&tag_ride(ride))
}

/// Remove every entity of the mod (used when a world opens, D20).
pub fn kill_all() -> String {
    kill_tag(TAG_ALL)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_have_four_decimals_a_point_and_no_exponent() {
        assert_eq!(num(1.0), "1.0000");
        assert_eq!(num(-2.5), "-2.5000");
        assert_eq!(num(0.00004), "0.0000");
        assert_eq!(num(123456.789012), "123456.7890");
        assert_eq!(num(1.0e-9), "0.0000");
        assert_eq!(num(1.0e9), "1000000000.0000");
    }

    #[test]
    fn minus_zero_is_written_as_plain_zero() {
        assert_eq!(num(-0.0), "0.0000");
        assert_eq!(num(-0.00001), "0.0000");
    }

    #[test]
    fn rail_summon_text() {
        // A quarter turn about Y: the quaternion is (0, sin 45°, 0, cos 45°).
        let q = DQuat::from_rotation_y(std::f64::consts::FRAC_PI_2);
        let text = summon_rail(
            DEMO,
            DVec3::new(1.5, 70.0, -2.25),
            q,
            DVec3::new(0.5, 0.15, 4.0),
        );
        assert_eq!(
            text,
            "summon minecraft:item_display 1.5000 70.0000 -2.2500 {Tags:[\"htp\",\"htp_demo\"],item:{id:\"htp:rail\",count:1},transformation:{left_rotation:[0.0000f,0.7071f,0.0000f,0.7071f],right_rotation:[0.0000f,0.0000f,0.0000f,1.0000f],translation:[0.0000f,0.0000f,0.0000f],scale:[0.5000f,0.1500f,4.0000f]}}"
        );
    }

    #[test]
    fn car_summon_text() {
        let text = summon_car(
            DEMO,
            0,
            DVec3::new(0.0, 64.0, 0.0),
            -90.0,
            12.5,
            3,
            DVec3::new(1.6, 0.9, 3.0),
        );
        assert_eq!(
            text,
            "summon minecraft:item_display 0.0000 64.0000 0.0000 {Tags:[\"htp\",\"htp_demo\",\"htp_car_demo_0\"],Rotation:[-90.0000f,12.5000f],item:{id:\"htp:car\",count:1},teleport_duration:3,transformation:{left_rotation:[0.0000f,0.0000f,0.0000f,1.0000f],right_rotation:[0.0000f,0.0000f,0.0000f,1.0000f],translation:[0.0000f,0.0000f,0.0000f],scale:[1.6000f,0.9000f,3.0000f]}}"
        );
    }

    #[test]
    fn car_move_text_has_yaw_and_pitch() {
        assert_eq!(
            move_car(DEMO, 0, DVec3::new(1.0, 2.0, 3.0), 180.0, -45.0),
            "tp @e[tag=htp_car_demo_0,limit=1] 1.0000 2.0000 3.0000 180.0000 -45.0000"
        );
    }

    #[test]
    fn seat_summon_and_move_text() {
        assert_eq!(
            summon_seat(DEMO, 1, DVec3::new(5.0, 70.5, 6.0)),
            vec![
                "summon minecraft:minecart 5.0000 70.5000 6.0000 {Tags:[\"htp\",\"htp_demo\",\"htp_seat_demo_1\"]}"
                    .to_string()
            ]
        );
        assert_eq!(
            move_seat(DEMO, 1, DVec3::new(5.0, 70.5, 6.0), -90.0, 0.0),
            "tp @e[tag=htp_seat_demo_1,limit=1] 5.0000 70.5000 6.0000 -90.0000 0.0000"
        );
    }

    #[test]
    fn action_bar_text() {
        assert_eq!(
            action_bar("Alex", "Speed: 54 km/h"),
            "title Alex actionbar {\"text\":\"Speed: 54 km/h\"}"
        );
    }

    #[test]
    fn fill_texts() {
        assert_eq!(
            fill([88, 73, -39], [97, 73, -38], PLATFORM_BLOCK),
            "fill 88 73 -39 97 73 -38 minecraft:smooth_stone"
        );
        assert_eq!(
            fill([88, 73, -39], [97, 73, -38], "minecraft:air"),
            "fill 88 73 -39 97 73 -38 minecraft:air"
        );
    }

    #[test]
    fn kill_texts() {
        assert_eq!(kill_all(), "kill @e[tag=htp]");
        assert_eq!(kill_ride(DEMO), "kill @e[tag=htp_demo]");
        assert_eq!(kill_tag("htp_seat_demo_0"), "kill @e[tag=htp_seat_demo_0]");
    }
}
