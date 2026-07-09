use rl_env::scenario::{Scenario, Terminal};

#[test]
fn loads_mirror_melee() {
    let sc = Scenario::load("scenarios/mirror_melee.ron").expect("parse");
    assert_eq!(sc.name, "mirror_melee");
    assert_eq!(sc.spot, (3200, 0, 3912));
    assert!(matches!(sc.terminal, Terminal::DeathOrTimeout(_)));
    // mirror: both sides identical
    assert_eq!(sc.sides[0].stats, sc.sides[1].stats);
    assert!(sc.sides[0].worn.iter().any(|o| o.contains("scimitar") || o.contains("whip") || o.contains("sword")));
    assert!(sc.sides[0].inventory.iter().any(|(o, _)| o.contains("shark") || o.contains("food") || o.contains("lobster")));
}
