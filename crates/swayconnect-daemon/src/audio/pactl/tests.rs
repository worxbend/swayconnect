use swayconnect_protocol::{AudioChange, DeviceKind};

use super::*;

const SINKS: &[u8] = include_bytes!("../../../tests/fixtures/pactl/sinks.json");
const SOURCES: &[u8] = include_bytes!("../../../tests/fixtures/pactl/sources.json");
const DEFAULT_SINK: &[u8] = include_bytes!("../../../tests/fixtures/pactl/default-sink.txt");
const DEFAULT_SOURCE: &[u8] = include_bytes!("../../../tests/fixtures/pactl/default-source.txt");
const HOSTILE_SINKS: &[u8] = include_bytes!("../../../tests/fixtures/pactl/hostile-sinks.json");
const HOSTILE_SOURCES: &[u8] = include_bytes!("../../../tests/fixtures/pactl/hostile-sources.json");
const NULL_SINKS: &[u8] = include_bytes!("../../../tests/fixtures/pactl/null-sinks.json");
const NULL_SOURCES: &[u8] = include_bytes!("../../../tests/fixtures/pactl/null-sources.json");

fn node(name: &str, description: &str, kind: DeviceKind, volume_pct: u16) -> Node {
    Node {
        name: name.to_owned(),
        description: description.to_owned(),
        kind,
        volume_pct,
        muted: false,
        available: true,
    }
}

#[test]
fn the_captured_lists_parse_and_hide_the_monitor() {
    let sinks = parse_list(Direction::Sink, SINKS).expect("sinks");
    assert_eq!(
        sinks,
        [node(
            "alsa_output.pci-0000_2b_00.4.analog-stereo",
            "Starship/Matisse HD Audio Controller Analog Stereo",
            DeviceKind::Speaker,
            15,
        )]
    );

    let sources = parse_list(Direction::Source, SOURCES).expect("sources");
    assert_eq!(
        sources,
        [Node {
            available: false,
            ..node(
                "alsa_input.pci-0000_2b_00.4.analog-stereo",
                "Starship/Matisse HD Audio Controller Analog Stereo",
                DeviceKind::Microphone,
                100,
            )
        }],
        "the monitor source is hidden; the mic port is unplugged"
    );
}

/// The sealed PipeWire of `just real` (`docs/M2C_PLAN.md` 4, R1): null sinks have no port,
/// and their monitors carry `device.class` = `monitor` and `monitor_source`.
#[test]
fn the_null_sink_capture_parses_and_hides_both_monitors() {
    let sinks = parse_list(Direction::Sink, NULL_SINKS).expect("sinks");
    let sink = |name: &str| {
        node(
            name,
            &format!("{name} Audio/Sink sink"),
            DeviceKind::Other,
            100,
        )
    };
    assert_eq!(sinks, [sink("sealed_sink_1"), sink("sealed_sink_2")]);

    let sources = parse_list(Direction::Source, NULL_SOURCES).expect("sources");
    let description = "sealed_source Audio/Source/Virtual sink";
    let source = node("sealed_source", description, DeviceKind::Other, 100);
    assert_eq!(sources, [source]);
}

#[test]
fn the_captured_defaults_parse() {
    assert_eq!(
        parse_default(DEFAULT_SINK).as_deref(),
        Some("alsa_output.pci-0000_2b_00.4.analog-stereo")
    );
    assert_eq!(
        parse_default(DEFAULT_SOURCE).as_deref(),
        Some("alsa_input.pci-0000_2b_00.4.analog-stereo")
    );
    assert_eq!(parse_default(b""), None);
    assert_eq!(parse_default(b"\n"), None);
    assert_eq!(parse_default(b"-evil\n"), None);
}

#[test]
fn hostile_sinks_are_dropped_cleaned_and_capped() {
    let sinks = parse_list(Direction::Sink, HOSTILE_SINKS).expect("sinks");
    let expected = [
        Node {
            muted: true,
            available: false,
            ..node(
                "bluez_output.00_00_00_00_00_01.1",
                "Head\u{FFFD}phones\u{FFFD}[31m",
                DeviceKind::Bluetooth,
                200,
            )
        },
        node(
            "alsa_output.pci-0000_01_00.1.hdmi-stereo",
            &"x".repeat(MAX_LABEL_SCALARS),
            DeviceKind::Hdmi,
            100,
        ),
        node("easyeffects_sink", "easyeffects_sink", DeviceKind::Other, 0),
        node(
            "alsa_output.usb-Foo-00.analog-stereo",
            "USB DAC",
            DeviceKind::Headphones,
            50,
        ),
        node("loud_sink_2", "Absurd volume", DeviceKind::Other, 1000),
    ];
    assert_eq!(sinks, expected);
}

#[test]
fn both_monitor_spellings_are_hidden_and_sources_get_their_kind() {
    let sources = parse_list(Direction::Source, HOSTILE_SOURCES).expect("sources");
    let expected = [
        Node {
            muted: true,
            ..node(
                "pulse_style_input",
                "PulseAudio input",
                DeviceKind::Microphone,
                60,
            )
        },
        node(
            "alsa_input.usb-Blue_Yeti-00.analog-stereo",
            "Yeti Stereo Microphone",
            DeviceKind::Usb,
            80,
        ),
    ];
    assert_eq!(sources, expected);
}

#[test]
fn output_that_is_not_an_array_is_an_error() {
    for bad in [
        &b"{}"[..],
        b"",
        b"Connection failure: Connection refused\n",
        b"[1,",
    ] {
        assert!(parse_list(Direction::Sink, bad).is_err(), "{bad:?}");
    }
    assert_eq!(parse_list(Direction::Sink, b"[]").expect("empty"), []);
}

#[test]
fn every_write_ends_the_options_before_the_command_word() {
    let cases = [
        (
            Direction::Sink,
            AudioChange::Volume(55),
            vec!["--", "set-sink-volume", "--server=x", "55%"],
        ),
        (
            Direction::Sink,
            AudioChange::Mute(true),
            vec!["--", "set-sink-mute", "--server=x", "1"],
        ),
        (
            Direction::Source,
            AudioChange::Mute(false),
            vec!["--", "set-source-mute", "--server=x", "0"],
        ),
        (
            Direction::Source,
            AudioChange::Volume(0),
            vec!["--", "set-source-volume", "--server=x", "0%"],
        ),
        (
            Direction::Sink,
            AudioChange::Default,
            vec!["--", "set-default-sink", "--server=x"],
        ),
        (
            Direction::Source,
            AudioChange::Default,
            vec!["--", "set-default-source", "--server=x"],
        ),
    ];
    for (direction, change, expected) in cases {
        assert_eq!(
            write_args(direction, "--server=x", change, 100),
            expected,
            "{direction:?} {change:?}"
        );
    }
}

#[test]
fn a_volume_is_clamped_to_the_server_maximum() {
    assert_eq!(
        write_args(Direction::Sink, "s", AudioChange::Volume(150), 100),
        ["--", "set-sink-volume", "s", "100%"]
    );
    assert_eq!(
        write_args(Direction::Sink, "s", AudioChange::Volume(150), 150),
        ["--", "set-sink-volume", "s", "150%"]
    );
}

#[test]
fn read_commands_are_fixed() {
    assert_eq!(list_args(Direction::Sink), ["-f", "json", "list", "sinks"]);
    assert_eq!(
        list_args(Direction::Source),
        ["-f", "json", "list", "sources"]
    );
    assert_eq!(default_args(Direction::Sink), ["get-default-sink"]);
    assert_eq!(default_args(Direction::Source), ["get-default-source"]);
    assert_eq!(SUBSCRIBE_ARGS, ["subscribe"]);
}

#[test]
fn only_device_events_count() {
    for line in [
        "Event 'change' on sink #59",
        "Event 'new' on source #60",
        "Event 'remove' on card #52",
        "Event 'change' on server #4294967295",
        "Ereignis »change« auf sink #59",
    ] {
        assert!(is_device_event(line), "{line}");
    }
    // Captured on this machine while a snapshot ran: pactl's own client events.
    for line in [
        "Event 'change' on client #176",
        "Event 'new' on client #177",
        "Event 'remove' on client #177",
        "Event 'new' on sink-input #90",
        "Event 'change' on source-output #91",
        "Event 'new' on module #3",
        "Got SIGINT, exiting.",
        "",
        "sink",
    ] {
        assert!(!is_device_event(line), "{line}");
    }
}
