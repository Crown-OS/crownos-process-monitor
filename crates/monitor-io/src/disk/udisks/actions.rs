use std::sync::Arc;

use zbus::Connection;

use super::SERVICE;
use super::objects::{ObjectIndex, no_options};
use crate::IoError;

const FILESYSTEM: &str = "org.freedesktop.UDisks2.Filesystem";
const DRIVE: &str = "org.freedesktop.UDisks2.Drive";

/// A `Drive` interface method the user may ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DriveAction {
    Eject,
    PowerOff,
}

impl DriveAction {
    const fn method(self) -> &'static str {
        match self {
            Self::Eject => "Eject",
            Self::PowerOff => "PowerOff",
        }
    }
}

pub(super) async fn unmount(
    connection: &Connection,
    index: &mut ObjectIndex,
    device: &Arc<str>,
) -> Result<(), IoError> {
    let (block, _) = index.lookup(connection, device).await?;
    connection
        .call_method(
            Some(SERVICE),
            block.as_str(),
            Some(FILESYSTEM),
            "Unmount",
            &(no_options(),),
        )
        .await?;
    Ok(())
}

pub(super) async fn drive_action(
    connection: &Connection,
    index: &mut ObjectIndex,
    device: &Arc<str>,
    action: DriveAction,
) -> Result<(), IoError> {
    let (_, drive) = index.lookup(connection, device).await?;
    let drive = drive.ok_or_else(|| IoError::UnknownDevice(Arc::clone(device)))?;
    connection
        .call_method(
            Some(SERVICE),
            drive.as_str(),
            Some(DRIVE),
            action.method(),
            &(no_options(),),
        )
        .await?;
    Ok(())
}
