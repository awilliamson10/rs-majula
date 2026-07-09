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

#[derive(Debug, Clone, PartialEq, Deserialize)]
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
pub fn stat_index(name: &str) -> Option<usize> {
    Some(match name {
        "attack" => 0, "defence" | "defense" => 1, "strength" => 2,
        "hitpoints" | "hp" => 3, "ranged" => 4, "prayer" => 5, "magic" => 6,
        _ => return None,
    })
}
