use std::collections::HashMap;
use std::sync::Arc;

use zbus::{Connection, OwnedObjectPath};

use super::objects::{BLOCK, ObjectIndex, block_name, managed_objects, no_options};
use super::{HealthTable, SERVICE, TableState};
use crate::disk::health::{self, DriveHealth, RawAtaAttribute};
use crate::util::dbus::{self, Properties};

const PARTITION: &str = "org.freedesktop.UDisks2.Partition";
const DRIVE_ATA: &str = "org.freedesktop.UDisks2.Drive.Ata";
const NVME_CONTROLLER: &str = "org.freedesktop.UDisks2.NVMe.Controller";

pub(super) async fn refresh(connection: &Connection, index: &mut ObjectIndex) -> HealthTable {
    let Ok(objects) = managed_objects(connection).await else {
        return HealthTable::unavailable();
    };
    index.rebuild(&objects);
    let mut drives = HashMap::new();
    for interfaces in objects.values() {
        let Some(block) = interfaces.get(BLOCK) else {
            continue;
        };
        if interfaces.contains_key(PARTITION) {
            continue;
        }
        let (Some(name), Some(drive_path)) = (block_name(block), dbus::object_path(block, "Drive"))
        else {
            continue;
        };
        let drive = objects
            .iter()
            .find(|(candidate, _)| candidate.as_str() == drive_path);
        let health = match drive {
            Some((drive_path, drive_interfaces)) => {
                drive_health(connection, drive_path, drive_interfaces).await
            }
            None => DriveHealth::Unavailable,
        };
        drives.insert(name, health);
    }
    HealthTable {
        state: TableState::Ready,
        drives,
    }
}

async fn drive_health(
    connection: &Connection,
    path: &OwnedObjectPath,
    interfaces: &HashMap<String, Properties>,
) -> DriveHealth {
    if let Some(controller) = interfaces.get(NVME_CONTROLLER) {
        let attributes = smart_attributes::<Properties>(connection, path, NVME_CONTROLLER)
            .await
            .unwrap_or_default();
        return DriveHealth::Nvme(Arc::new(health::nvme_health(controller, &attributes)));
    }
    let Some(ata) = interfaces.get(DRIVE_ATA) else {
        return DriveHealth::Unavailable;
    };
    let ready = dbus::value(ata, "SmartSupported").unwrap_or(false)
        && dbus::value(ata, "SmartEnabled").unwrap_or(false);
    let attributes = if ready {
        smart_attributes::<Vec<RawAtaAttribute>>(connection, path, DRIVE_ATA)
            .await
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    DriveHealth::Ata(Arc::new(health::ata_health(ata, attributes)))
}

async fn smart_attributes<Attributes>(
    connection: &Connection,
    path: &OwnedObjectPath,
    interface: &str,
) -> zbus::Result<Attributes>
where
    Attributes: for<'de> zbus::wire::DynamicDeserialize<'de>,
{
    let reply = connection
        .call_method(
            Some(SERVICE),
            path.as_str(),
            Some(interface),
            "SmartGetAttributes",
            &(no_options(),),
        )
        .await?;
    reply.body().deserialize()
}
