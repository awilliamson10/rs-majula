use serde::Deserialize;
use std::fmt;

#[derive(Debug, Clone, Deserialize)]
pub struct Scenario {
    pub name: String,
    pub spot: (u16, u8, u16),          // (x, level, z) -> CoordGrid::new(x, level, z)
    pub seed: u64,
    #[serde(default)]
    pub start_jitter: u8,
    pub terminal: Terminal,
    pub sides: [Loadout; 2],           // [0]=pker, [1]=opponent
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub enum Terminal {
    Death,
    Timeout(u32),
    DeathOrTimeout(u32),
}

#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
pub struct Loadout {
    pub stats: Vec<(String, u8)>,      // (stat debugname, level) e.g. ("strength", 99)
    pub worn: Vec<String>,             // obj debugnames to equip
    pub inventory: Vec<(String, u32)>, // (obj debugname, count)
    /// (varp debugname, value) pairs applied to the spawned player, e.g.
    /// `("zanaris", 6)` to mark the Lost City quest complete so
    /// quest-gated `OpHeld` wields (e.g. `dragon_dagger`) aren't silently
    /// refused. Defaults empty so existing scenarios stay valid.
    #[serde(default)]
    pub vars: Vec<(String, i32)>,
}

#[derive(Debug)]
pub enum ScenarioError { Io(std::io::Error), Parse(String) }
impl fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self { Self::Io(e) => write!(f, "io: {e}"), Self::Parse(e) => write!(f, "parse: {e}") }
    }
}

impl Scenario {
    pub fn load(path: &str) -> Result<Scenario, ScenarioError> {
        let text = std::fs::read_to_string(path).map_err(ScenarioError::Io)?;
        ron::from_str(&text).map_err(|e| ScenarioError::Parse(e.to_string()))
    }
}

/// OSRS stat order used by `stats.levels`: 0=Attack 1=Defence 2=Strength
/// 3=Hitpoints 4=Ranged 5=Prayer 6=Magic ... (matches rs-stat PlayerStat).
/// Resolves a stat debugname to its ENGINE index.
///
/// ★★ NOT `stat.constant`'s NUMBER. `content/274/scripts/player/configs/
/// stat.constant` numbers the skills 1..19 in the interface's order and says
/// `^woodcutting = 18`; `StatBlock` is indexed by `PlayerStat`
/// (`rs-pack/src/types.rs:952`), where `Woodcutting = 8`. Reading a stat by the
/// constant's number returns a different skill's level, silently, and looks
/// like a training bug.
///
/// ★★ THE GUARD IS REQUIRED, NOT DEFENSIVE. `PlayerStat::from_config_str`
/// PANICS on an unrecognised name (`types.rs:999`), so an unknown name has to
/// be rejected BEFORE the call. Without this a typo in a task file aborts the
/// process instead of being reported alongside every other unresolved name --
/// and `Task::resolve` exists precisely to report them all at once.
///
/// ★ Two aliases are kept for scenarios already on disk (`mirror_melee.ron`):
/// `defense` and `hp`. `from_config_str` knows neither.
pub fn stat_index(name: &str) -> Option<usize> {
    use rs_pack::types::PlayerStat;
    let canonical = match name {
        "defense" => "defence",
        "hp" => "hitpoints",
        other => other,
    };
    if !KNOWN_STATS.contains(&canonical) {
        return None;
    }
    Some(PlayerStat::from_config_str(canonical) as usize)
}

/// Every name `PlayerStat::from_config_str` accepts. Kept in sync with it by
/// `stat_index_covers_the_non_combat_skills`; the list exists because that
/// function panics rather than returning an Option.
const KNOWN_STATS: &[&str] = &[
    "attack", "defence", "strength", "hitpoints", "ranged", "prayer", "magic",
    "cooking", "woodcutting", "fletching", "fishing", "firemaking", "crafting",
    "smithing", "mining", "herblore", "agility", "thieving", "stat18", "stat19",
    "runecraft",
];

