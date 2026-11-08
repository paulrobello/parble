//! `Sim::sensors` (the render debug overlay's source) reports every showcase sensor, sees marbles
//! inside them, and reading it leaves the run's hashes unchanged.

use parble_sim::Sim;

const TICKS: u32 = 2400;

#[test]
fn showcase_sensors_report_occupancy_without_changing_the_run() {
    let mut plain = Sim::mech_showcase_scene();
    let mut read = Sim::mech_showcase_scene();
    assert_eq!(read.sensors().len(), 3);
    let mut first_occupied = None;
    for t in 1..=TICKS {
        plain.tick(1);
        read.tick(1);
        let sensors = read.sensors();
        assert_eq!(sensors.len(), 3);
        if first_occupied.is_none() && sensors.iter().any(|s| s.occupants > 0) {
            first_occupied = Some(t);
        }
    }
    eprintln!("first occupied sensor at tick {first_occupied:?}");
    assert!(first_occupied.is_some(), "no marble ever entered a sensor");
    assert_eq!(plain.trajectory_hash(), read.trajectory_hash());
    assert_eq!(plain.mechanism_hash(), read.mechanism_hash());
}
