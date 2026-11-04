//! Raw method calls over a zbus connection: `Properties.GetAll`/`Get` and
//! helpers that pick typed values out of the returned variants.

use std::collections::HashMap;

use zbus::{Connection, OwnedVariant, Variant};

const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const BUS_NAME: &str = "org.freedesktop.DBus";
const BUS_PATH: &str = "/org/freedesktop/DBus";

pub(crate) type Properties = HashMap<String, OwnedVariant>;

pub(crate) async fn get_all(
    connection: &Connection,
    destination: &str,
    path: &str,
    interface: &str,
) -> zbus::Result<Properties> {
    let reply = connection
        .call_method(
            Some(destination),
            path,
            Some(PROPERTIES),
            "GetAll",
            &(interface,),
        )
        .await?;
    reply.body().deserialize()
}

pub(crate) async fn get(
    connection: &Connection,
    destination: &str,
    path: &str,
    interface: &str,
    name: &str,
) -> zbus::Result<OwnedVariant> {
    let reply = connection
        .call_method(
            Some(destination),
            path,
            Some(PROPERTIES),
            "Get",
            &(interface, name),
        )
        .await?;
    reply.body().deserialize()
}

/// True when `name` currently has an owner. Unlike a method call, this never
/// activates a service that is merely activatable.
pub(crate) async fn name_has_owner(connection: &Connection, name: &str) -> bool {
    let Ok(reply) = connection
        .call_method(
            Some(BUS_NAME),
            BUS_PATH,
            Some(BUS_NAME),
            "NameHasOwner",
            &(name,),
        )
        .await
    else {
        return false;
    };
    reply.body().deserialize::<bool>().unwrap_or(false)
}

/// A basic-typed property (`u32`, `bool`, `u64`…).
pub(crate) fn value<'a, T>(properties: &'a Properties, name: &str) -> Option<T>
where
    T: TryFrom<&'a OwnedVariant>,
{
    properties
        .get(name)
        .and_then(|variant| T::try_from(variant).ok())
}

pub(crate) fn string<'a>(properties: &'a Properties, name: &str) -> Option<&'a str> {
    value::<&str>(properties, name).filter(|text| !text.is_empty())
}

pub(crate) fn object_path<'a>(properties: &'a Properties, name: &str) -> Option<&'a str> {
    match properties.get(name).map(|variant| unwrap_variant(variant)) {
        Some(Variant::ObjectPath(path)) if path.as_str() != "/" => Some(path.as_str()),
        _ => None,
    }
}

/// An `ay` property, e.g. an SSID or a NUL-terminated device path.
pub(crate) fn bytes(variant: &Variant<'_>) -> Option<Vec<u8>> {
    let Variant::Array(array) = unwrap_variant(variant) else {
        return None;
    };
    array
        .inner()
        .iter()
        .map(|element| match element {
            Variant::U8(byte) => Some(*byte),
            _ => None,
        })
        .collect()
}

/// An `as` property.
pub(crate) fn strings(variant: &Variant<'_>) -> Vec<String> {
    let Variant::Array(array) = unwrap_variant(variant) else {
        return Vec::new();
    };
    array
        .inner()
        .iter()
        .filter_map(|element| match element {
            Variant::Str(text) => Some(text.as_str().to_owned()),
            _ => None,
        })
        .collect()
}

/// The elements of an array of structures, each as its field list.
pub(crate) fn structures<'v>(variant: &'v Variant<'v>) -> impl Iterator<Item = &'v [Variant<'v>]> {
    let elements: &[Variant<'_>] = match unwrap_variant(variant) {
        Variant::Array(array) => array.inner(),
        _ => &[],
    };
    elements.iter().filter_map(|element| match element {
        Variant::Structure(structure) => Some(structure.fields()),
        _ => None,
    })
}

pub(crate) fn unwrap_variant<'v>(variant: &'v Variant<'v>) -> &'v Variant<'v> {
    match variant {
        Variant::Variant(inner) => inner,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_byte_arrays_and_paths() {
        let ssid = Variant::new(vec![b'n', b'e', b't']);
        assert_eq!(bytes(&ssid), Some(b"net".to_vec()));
        let mut properties = Properties::new();
        properties.insert("Bitrate".into(), OwnedVariant::from(866_700_u32));
        assert_eq!(value::<u32>(&properties, "Bitrate"), Some(866_700));
        assert_eq!(value::<bool>(&properties, "Bitrate"), None);
    }
}
