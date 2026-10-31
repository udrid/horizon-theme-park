//! Saving (D20): one JSON file per ride, plus `park.json`, inside the world's own folder.
//!
//! ```text
//! <world>/horizon-theme-park/park.json          {"format":1, "session":3, "rides":["thunder", ...]}
//! <world>/horizon-theme-park/rides/<id>.json    {"format":1, "id":"thunder", "nodes":[{"pos":[x,y,z], ...}], ...}
//! ```
//!
//! The ride files are the ONE true copy of a ride (nothing in the world's blocks says anything
//! about it). So saving is careful:
//!   * we write `<file>.tmp` first, copy the old file to `<file>.bak`, and only then swap the new
//!     one in. A crash half way leaves the old file whole, never a half-written one;
//!   * a file whose `"format"` is newer than this mod knows is never loaded and never overwritten
//!     (it was written by a newer version of the mod, and we must not damage it);
//!   * one broken file loses only that one ride.
//!
//! This is the only file in park-core that touches the disk (rule 5). Positions are plain
//! `[f64; 3]` arrays because Horizon's `glam` has no serde support (rule 10), and angles are in
//! degrees (D11).

use crate::coaster::elements::{BRAKE_DECEL, BRAKE_TARGET, DWELL, LIFT_SPEED, STATION_PUSH};
use crate::coaster::physics::{CAR_SPACING, Physics};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The file format this version of the mod writes and understands.
pub const FORMAT: u32 = 1;
/// The mod's folder inside the world folder.
pub const FOLDER: &str = "horizon-theme-park";
/// The sub-folder with one file per ride.
pub const RIDES: &str = "rides";

/// What the track does from a node to the next one (D63).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Track,
    Lift,
    Brake,
    Station,
}

/// One point the track passes through.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct NodeFile {
    /// Where the rail passes, in world blocks: `[x, y, z]`.
    pub pos: [f64; 3],
    /// Bank angle in degrees. Not used until M5, kept so old files stay valid.
    #[serde(default)]
    pub roll: f64,
    /// What the track does from this node to the next.
    pub kind: Kind,
}

/// The numbers that tune a ride (S5). Anything missing from a file takes the default.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct SettingsFile {
    pub gravity_scale: f64,
    pub friction: f64,
    pub drag: f64,
    pub lift_speed: f64,
    pub brake_target: f64,
    pub brake_decel: f64,
    pub station_push: f64,
    pub dwell: f64,
}

impl Default for SettingsFile {
    fn default() -> Self {
        let physics = Physics::default();
        SettingsFile {
            gravity_scale: physics.gravity_scale,
            friction: physics.friction,
            drag: physics.drag,
            lift_speed: LIFT_SPEED,
            brake_target: BRAKE_TARGET,
            brake_decel: BRAKE_DECEL,
            station_push: STATION_PUSH,
            dwell: DWELL,
        }
    }
}

/// One train: how many cars, and how far apart their middles are.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TrainFile {
    pub cars: u32,
    pub spacing: f64,
}

/// Everything saved about one ride. Geometry and stats are rebuilt on load.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RideFile {
    pub format: u32,
    pub id: String,
    /// The last node joins the first (always true in M3, C4).
    pub closed: bool,
    pub settings: SettingsFile,
    pub nodes: Vec<NodeFile>,
    pub trains: Vec<TrainFile>,
}

impl RideFile {
    /// A new ride with default settings and one 3-car train (D55).
    pub fn new(id: &str, nodes: Vec<NodeFile>) -> RideFile {
        RideFile {
            format: FORMAT,
            id: id.to_string(),
            closed: true,
            settings: SettingsFile::default(),
            nodes,
            trains: vec![TrainFile {
                cars: 3,
                spacing: CAR_SPACING,
            }],
        }
    }
}

/// The park's own file: the session number (D66) and the list of rides.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ParkFile {
    pub format: u32,
    /// Bumped at every world open; it is the number in the session tag `htp_s<n>`.
    pub session: u64,
    pub rides: Vec<String>,
}

impl Default for ParkFile {
    fn default() -> Self {
        ParkFile {
            format: FORMAT,
            session: 0,
            rides: Vec::new(),
        }
    }
}

/// Why a file could not be used.
#[derive(Clone, Debug, PartialEq)]
pub enum LoadError {
    /// Written by a newer version of the mod. Left untouched.
    Newer { found: u64, known: u32 },
    /// Not valid JSON, or the wrong shape.
    Broken(String),
    /// The disk said no.
    Unreadable(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Newer { found, known } => write!(
                f,
                "it was saved by a newer version of the mod (format {found}, this one knows {known}); it was left untouched"
            ),
            LoadError::Broken(why) => write!(f, "the file is damaged ({why}); it was left untouched"),
            LoadError::Unreadable(why) => write!(f, "the file could not be read ({why})"),
        }
    }
}

/// `<world>/horizon-theme-park/rides`
pub fn rides_dir(world: &Path) -> PathBuf {
    world.join(FOLDER).join(RIDES)
}

/// `<world>/horizon-theme-park/park.json`
pub fn park_path(world: &Path) -> PathBuf {
    world.join(FOLDER).join("park.json")
}

/// `<world>/horizon-theme-park/rides/<id>.json`
pub fn ride_path(world: &Path, id: &str) -> PathBuf {
    rides_dir(world).join(format!("{id}.json"))
}

/// The same path with `suffix` stuck on the end: `thunder.json` -> `thunder.json.tmp`.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Write `text` to `path` the safe way (D20): into `<path>.tmp` first, the old file copied to
/// `<path>.bak`, and only then the new file renamed over the old one. The old file is never
/// missing and never half written.
pub fn write_safely(path: &Path, text: &str) -> io::Result<()> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    let tmp = with_suffix(path, ".tmp");
    fs::write(&tmp, text)?;
    if path.exists() {
        fs::copy(path, with_suffix(path, ".bak"))?;
    }
    fs::rename(&tmp, path)
}

/// Read the `format` number out of JSON text, before anything else is looked at.
fn read_format(text: &str) -> Result<(serde_json::Value, u64), LoadError> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| LoadError::Broken(e.to_string()))?;
    let format = value
        .get("format")
        .and_then(|f| f.as_u64())
        .ok_or_else(|| LoadError::Broken("there is no \"format\" number".to_string()))?;
    if format > FORMAT as u64 {
        return Err(LoadError::Newer {
            found: format,
            known: FORMAT,
        });
    }
    Ok((value, format))
}

/// Turn the text of a ride file into a ride. A newer format is refused first; then the shape is checked.
pub fn parse_ride(text: &str) -> Result<RideFile, LoadError> {
    let (value, _) = read_format(text)?;
    let ride: RideFile =
        serde_json::from_value(value).map_err(|e| LoadError::Broken(e.to_string()))?;
    if ride
        .nodes
        .iter()
        .any(|n| n.pos.iter().any(|c| !c.is_finite()))
    {
        return Err(LoadError::Broken("a node has a position that is not a number".to_string()));
    }
    Ok(ride)
}

/// Read one ride file.
pub fn load_ride(path: &Path) -> Result<RideFile, LoadError> {
    let text = fs::read_to_string(path).map_err(|e| LoadError::Unreadable(e.to_string()))?;
    parse_ride(&text)
}

/// Save a ride. Refuses (and writes nothing) when the file on disk is from a newer version.
pub fn save_ride(world: &Path, ride: &RideFile) -> Result<(), String> {
    let path = ride_path(world, &ride.id);
    if let Ok(old) = fs::read_to_string(&path)
        && let Err(LoadError::Newer { found, known }) = read_format(&old)
    {
        return Err(format!(
            "{}.json was saved by a newer version of the mod (format {found}, this one knows {known}); it was not overwritten",
            ride.id
        ));
    }
    let text = serde_json::to_string_pretty(ride).map_err(|e| e.to_string())?;
    write_safely(&path, &text).map_err(|e| format!("could not save {}.json: {e}", ride.id))
}

/// Does a file for this ride already exist (good, newer, or broken)?
pub fn ride_exists(world: &Path, id: &str) -> bool {
    ride_path(world, id).exists()
}

/// Put a ride aside: `<id>.json` becomes `<id>.json.deleted` (S3 keeps it, nothing is lost).
pub fn delete_ride(world: &Path, id: &str) -> Result<(), String> {
    let path = ride_path(world, id);
    fs::rename(&path, with_suffix(&path, ".deleted"))
        .map_err(|e| format!("could not delete {id}.json: {e}"))
}

/// What loading every ride gave: the good ones, and a plain sentence for each one that failed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Loaded {
    pub rides: Vec<RideFile>,
    pub problems: Vec<String>,
    /// Ride ids whose files exist but could not be used. They are never overwritten.
    pub locked: Vec<String>,
}

/// Load every `rides/*.json`, in name order. A leftover `.tmp`, a `.bak` and a `.deleted` file
/// are not rides and are ignored. One bad file only loses its own ride.
pub fn load_all(world: &Path) -> Loaded {
    let mut out = Loaded::default();
    let Ok(entries) = fs::read_dir(rides_dir(world)) else {
        return out;
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();
    for path in paths {
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        match load_ride(&path) {
            Ok(ride) if ride.id == stem => out.rides.push(ride),
            Ok(ride) => {
                out.problems.push(format!(
                    "ride {stem}: the file says its name is \"{}\"; it was skipped",
                    ride.id
                ));
                out.locked.push(stem);
            }
            Err(error) => {
                out.problems.push(format!("ride {stem}: {error}"));
                out.locked.push(stem);
            }
        }
    }
    out
}

/// Read `park.json`. No file yet is fine (a fresh default); a newer or damaged file is an error.
pub fn load_park(world: &Path) -> Result<ParkFile, LoadError> {
    let text = match fs::read_to_string(park_path(world)) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(ParkFile::default()),
        Err(e) => return Err(LoadError::Unreadable(e.to_string())),
    };
    let (value, _) = read_format(&text)?;
    serde_json::from_value(value).map_err(|e| LoadError::Broken(e.to_string()))
}

/// Write `park.json` the safe way.
pub fn save_park(world: &Path, park: &ParkFile) -> Result<(), String> {
    let text = serde_json::to_string_pretty(park).map_err(|e| e.to_string())?;
    write_safely(&park_path(world), &text).map_err(|e| format!("could not save park.json: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty scratch folder just for one test (same name = same folder, cleared first).
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("htp-test-save-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample(id: &str) -> RideFile {
        let kinds = [Kind::Station, Kind::Lift, Kind::Track, Kind::Brake];
        let nodes = kinds
            .iter()
            .enumerate()
            .map(|(i, &kind)| NodeFile {
                pos: [i as f64 * 5.5 + 0.5, 70.25, -3.0 - i as f64],
                roll: 0.0,
                kind,
            })
            .collect();
        let mut ride = RideFile::new(id, nodes);
        ride.settings.friction = 0.02;
        ride.trains[0].cars = 5;
        ride
    }

    // Test 1.
    #[test]
    fn a_ride_saved_and_loaded_is_equal() {
        let world = scratch("roundtrip");
        let ride = sample("thunder");
        save_ride(&world, &ride).unwrap();
        let back = load_ride(&ride_path(&world, "thunder")).unwrap();
        assert_eq!(back, ride);
        // And through load_all.
        let all = load_all(&world);
        assert_eq!(all.rides, vec![ride]);
        assert!(all.problems.is_empty());
    }

    #[test]
    fn positions_are_written_as_plain_arrays_and_kinds_as_words() {
        let text = serde_json::to_string(&sample("a")).unwrap();
        assert!(text.contains("\"pos\":[0.5,70.25,-3.0]"), "{text}");
        assert!(text.contains("\"kind\":\"station\""), "{text}");
        assert!(text.contains("\"format\":1"), "{text}");
    }

    #[test]
    fn missing_settings_take_their_defaults() {
        let text = r#"{"format":1,"id":"x","closed":true,"settings":{"friction":0.5},"nodes":[],"trains":[]}"#;
        let ride = parse_ride(text).unwrap();
        assert_eq!(ride.settings.friction, 0.5);
        assert_eq!(ride.settings.lift_speed, LIFT_SPEED);
    }

    // Test 2.
    #[test]
    fn a_newer_format_is_refused_and_left_untouched() {
        let world = scratch("newer");
        let path = ride_path(&world, "future");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let newer = r#"{"format":2,"id":"future","something":"new"}"#;
        fs::write(&path, newer).unwrap();

        assert_eq!(
            load_ride(&path),
            Err(LoadError::Newer {
                found: 2,
                known: FORMAT
            })
        );
        let loaded = load_all(&world);
        assert!(loaded.rides.is_empty());
        assert_eq!(loaded.locked, vec!["future"]);
        assert!(loaded.problems[0].contains("newer version"));

        // Saving a ride of the same name must not overwrite it.
        let error = save_ride(&world, &sample("future")).unwrap_err();
        assert!(error.contains("not overwritten"), "{error}");
        assert_eq!(fs::read_to_string(&path).unwrap(), newer);
        assert!(!with_suffix(&path, ".tmp").exists());
    }

    // Test 3.
    #[test]
    fn a_leftover_tmp_is_ignored_and_the_bak_survives_a_failed_write() {
        let world = scratch("crash");
        let ride = sample("loop");
        save_ride(&world, &ride).unwrap();
        let path = ride_path(&world, "loop");

        // A crash left a half-written .tmp behind: it is not a ride, and it does not matter.
        fs::write(with_suffix(&path, ".tmp"), "{\"format\":1,\"id\":\"lo").unwrap();
        let loaded = load_all(&world);
        assert_eq!(loaded.rides, vec![ride.clone()]);
        assert!(loaded.problems.is_empty());

        // A second save keeps the first version as .bak.
        let mut v2 = ride.clone();
        v2.trains[0].cars = 7;
        save_ride(&world, &v2).unwrap();
        assert_eq!(load_ride(&with_suffix(&path, ".bak")).unwrap(), ride);
        assert_eq!(load_ride(&path).unwrap(), v2);

        // A write that fails (the .tmp spot is blocked by a folder) touches neither file.
        fs::create_dir(with_suffix(&path, ".tmp")).unwrap();
        let mut v3 = ride.clone();
        v3.trains[0].cars = 2;
        assert!(save_ride(&world, &v3).is_err());
        assert_eq!(load_ride(&path).unwrap(), v2);
        assert_eq!(load_ride(&with_suffix(&path, ".bak")).unwrap(), ride);
    }

    // Test 4.
    #[test]
    fn a_broken_file_loses_only_its_own_ride() {
        let world = scratch("broken");
        save_ride(&world, &sample("a-good")).unwrap();
        save_ride(&world, &sample("c-good")).unwrap();
        fs::write(ride_path(&world, "b-bad"), "{ this is not json").unwrap();
        fs::write(ride_path(&world, "d-nonumber"), r#"{"id":"d-nonumber"}"#).unwrap();

        let loaded = load_all(&world);
        let ids: Vec<&str> = loaded.rides.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["a-good", "c-good"]);
        assert_eq!(loaded.problems.len(), 2);
        assert!(loaded.problems[0].starts_with("ride b-bad: the file is damaged"));
        assert_eq!(loaded.locked, vec!["b-bad", "d-nonumber"]);
        // The broken file is still there, unchanged.
        assert_eq!(
            fs::read_to_string(ride_path(&world, "b-bad")).unwrap(),
            "{ this is not json"
        );
    }

    #[test]
    fn deleting_keeps_the_file_as_deleted() {
        let world = scratch("delete");
        save_ride(&world, &sample("gone")).unwrap();
        delete_ride(&world, "gone").unwrap();
        assert!(!ride_exists(&world, "gone"));
        assert!(with_suffix(&ride_path(&world, "gone"), ".deleted").exists());
        assert!(load_all(&world).rides.is_empty());
    }

    #[test]
    fn park_json_round_trips_and_a_missing_one_is_a_fresh_default() {
        let world = scratch("park");
        assert_eq!(load_park(&world), Ok(ParkFile::default()));
        let park = ParkFile {
            format: FORMAT,
            session: 4,
            rides: vec!["a".into(), "b".into()],
        };
        save_park(&world, &park).unwrap();
        assert_eq!(load_park(&world), Ok(park));
        fs::write(park_path(&world), r#"{"format":9}"#).unwrap();
        assert!(matches!(load_park(&world), Err(LoadError::Newer { .. })));
    }
}
