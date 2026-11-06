use super::*;
use cpal::ErrorKind;

#[test]
fn only_errors_that_end_the_stream_are_fatal() {
    for kind in [ErrorKind::Xrun, ErrorKind::DeviceChanged, ErrorKind::RealtimeDenied] {
        assert!(!fatal(kind), "{kind:?}");
    }
    for kind in [
        ErrorKind::DeviceNotAvailable,
        ErrorKind::StreamInvalidated,
        ErrorKind::PermissionDenied,
        ErrorKind::BackendError,
        ErrorKind::Other,
    ] {
        assert!(fatal(kind), "{kind:?}");
    }
}
