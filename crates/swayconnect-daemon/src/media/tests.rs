use std::ffi::OsStr;
use std::path::Path;

use super::*;

#[test]
fn the_bus_address_is_the_environment_value_when_set() {
    let address = session_bus_address(
        Some(OsStr::new("unix:path=/run/user/1000/bus,guid=00")),
        Some(Path::new("/elsewhere")),
    );
    assert_eq!(
        address.as_deref(),
        Some("unix:path=/run/user/1000/bus,guid=00")
    );
}

#[test]
fn without_the_environment_value_the_bus_is_in_the_runtime_dir() {
    for empty in [None, Some(OsStr::new(""))] {
        let address = session_bus_address(empty, Some(Path::new("/run/user/1000")));
        assert_eq!(address.as_deref(), Some("unix:path=/run/user/1000/bus"));
    }
}

#[test]
fn the_runtime_dir_path_is_escaped_for_the_address_syntax() {
    let address = session_bus_address(None, Some(Path::new("/tmp/a b,c=d")));
    assert_eq!(address.as_deref(), Some("unix:path=/tmp/a%20b%2Cc%3Dd/bus"));
}

#[test]
fn no_value_and_no_absolute_runtime_dir_means_no_bus() {
    assert_eq!(session_bus_address(None, None), None);
    assert_eq!(session_bus_address(None, Some(Path::new("run/user"))), None);
}

#[test]
fn each_refusal_has_the_spec_error_code() {
    let cases = [
        (MediaError::NotFound, ErrorCode::NotFound),
        (MediaError::UnsupportedTarget, ErrorCode::UnsupportedTarget),
        (
            MediaError::Backend(BackendError::Unavailable("gone".into())),
            ErrorCode::BackendUnavailable,
        ),
        (
            MediaError::Backend(BackendError::Timeout),
            ErrorCode::Timeout,
        ),
    ];
    for (error, code) in cases {
        assert_eq!(error.code(), code, "{error:?}");
    }
}
