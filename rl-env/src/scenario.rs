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
