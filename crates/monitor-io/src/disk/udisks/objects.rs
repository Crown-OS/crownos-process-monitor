use std::collections::HashMap;
use std::sync::Arc;

use zbus::{Connection, OwnedObjectPath, Variant};

use super::SERVICE;
use crate::IoError;
use crate::util::dbus::{self, Properties};
use crate::util::text::file_name;

const ROOT: &str = "/org/freedesktop/UDisks2";
const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";
pub(super) const BLOCK: &str = "org.freedesktop.UDisks2.Block";

pub(super) type ManagedObjects = HashMap<OwnedObjectPath, HashMap<String, Properties>>;

/// Block device kernel name → its UDisks2 block and drive objects.
#[derive(Default)]
pub(super) struct ObjectIndex {
    blocks: HashMap<Arc<str>, (OwnedObjectPath, Option<OwnedObjectPath>)>,
}

impl ObjectIndex {
    pub(super) fn rebuild(&mut self, objects: &ManagedObjects) {
        self.blocks = objects
            .iter()
            .filter_map(|(path, interfaces)| {
                let block = interfaces.get(BLOCK)?;
                let drive = dbus::object_path(block, "Drive")
                    .and_then(|drive| OwnedObjectPath::try_from(drive).ok());
                Some((block_name(block)?, (path.clone(), drive)))
            })
            .collect();
    }

    pub(super) async fn lookup(
        &mut self,
        connection: &Connection,
        device: &Arc<str>,
    ) -> Result<(OwnedObjectPath, Option<OwnedObjectPath>), IoError> {
        if !self.blocks.contains_key(device) {
            self.rebuild(&managed_objects(connection).await?);
        }
        self.blocks
            .get(device)
            .cloned()
            .ok_or_else(|| IoError::UnknownDevice(Arc::clone(device)))
    }
}

pub(super) async fn managed_objects(connection: &Connection) -> zbus::Result<ManagedObjects> {
    let reply = connection
        .call_method(
            Some(SERVICE),
            ROOT,
            Some(OBJECT_MANAGER),
            "GetManagedObjects",
            &(),
        )
        .await?;
    reply.body().deserialize()
}

pub(super) fn no_options() -> HashMap<&'static str, Variant<'static>> {
    HashMap::new()
}

/// `Block.Device` is a NUL-terminated `ay` such as `/dev/nvme0n1`.
pub(super) fn block_name(block: &Properties) -> Option<Arc<str>> {
    let bytes = dbus::bytes(block.get("Device")?)?;
    let path = std::str::from_utf8(bytes.strip_suffix(&[0]).unwrap_or(&bytes)).ok()?;
    file_name(std::path::Path::new(path)).map(Arc::from)
}
