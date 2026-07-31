use rl_env::tape::{digest, TapeReader, TapeWriter};

#[test]
fn tape_round_trips_through_bytes() {
    let mut w = TapeWriter::new(1234);
    w.record_tick(0, &[vec![1, 2, 3], vec![4, 5]]);
    w.record_tick(1, &[vec![6]]);
    let bytes = w.finish();

    let t = TapeReader::parse(&bytes).expect("parses");
    assert_eq!(t.seed, 1234);
    assert_eq!(t.ticks.len(), 2);
    assert_eq!(t.ticks[0].tick, 0);
    assert_eq!(t.ticks[0].bytes, vec![1, 2, 3, 4, 5]);
    assert_eq!(t.ticks[1].bytes, vec![6]);
}

#[test]
fn digest_changes_when_any_byte_changes() {
    let mut a = TapeWriter::new(1);
    a.record_tick(0, &[vec![1, 2, 3]]);
    let ta = TapeReader::parse(&a.finish()).unwrap();

    let mut b = TapeWriter::new(1);
    b.record_tick(0, &[vec![1, 2, 4]]);
    let tb = TapeReader::parse(&b.finish()).unwrap();

    assert_ne!(digest(&ta), digest(&tb), "digest must be sensitive to payload");
}

#[test]
fn parse_rejects_a_bad_magic() {
    assert!(TapeReader::parse(b"NOPE0000________").is_err());
}
