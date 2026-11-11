use crate::format::DeviceFormat;

#[derive(Debug, thiserror::Error)]
pub enum OutputError {
    #[error("audio backend error: {0}")]
    Backend(String),
    #[error("output device not found: {0}")]
    DeviceNotFound(String),
    #[error("the device is in use by another application")]
    DeviceInUse,
    /// The device disappeared or stopped working mid-stream: unplugged, disabled, invalidated
    /// by a system sleep, or reconfigured by another application.
    #[error("the output device was lost: {0}")]
    DeviceLost(String),
    #[error(
        "exclusive mode is not allowed on this device; enable \"Allow applications to take \
         exclusive control of this device\" in the Windows sound properties"
    )]
    ExclusiveNotAllowed,
    #[error("the device does not support {requested}; supported formats: {}", format_list(.supported))]
    UnsupportedFormat {
        requested: String,
        supported: Vec<DeviceFormat>,
    },
}

fn format_list(formats: &[DeviceFormat]) -> String {
    if formats.is_empty() {
        return "none reported".to_string();
    }
    formats
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}
