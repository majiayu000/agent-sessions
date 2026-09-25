use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value, value::RawValue};
use std::borrow::Cow;

fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<&'de RawValue>, D::Error> {
    <&RawValue>::deserialize(d).map(Some)
}
macro_rules! fields {
    ($name:ident { $($field:ident => $wire:literal),* $(,)? })=> {
        #[derive(Deserialize)]
        pub(super) struct $name<'a> {
            $(#[serde(rename=$wire,default,borrow,deserialize_with="present")]
            pub(super) $field:Option<&'a RawValue>,)*
        }
    }
}
#[derive(Deserialize)]
pub(super) struct Header<'a> {
    #[serde(rename = "type", default, borrow, deserialize_with = "present")]
    kind: Option<&'a RawValue>,
    #[serde(default, borrow, deserialize_with = "present")]
    pub(super) timestamp: Option<&'a RawValue>,
    #[serde(default, borrow, deserialize_with = "present")]
    pub(super) created_at: Option<&'a RawValue>,
    #[serde(rename = "createdAt", default, borrow, deserialize_with = "present")]
    pub(super) created_at_camel: Option<&'a RawValue>,
    #[serde(default, borrow, deserialize_with = "present")]
    pub(super) uuid: Option<&'a RawValue>,
    #[serde(borrow)]
    pub payload: Option<Payload<'a>>,
}
fields!(Payload {
    kind=>"type",id=>"id",cwd=>"cwd",model=>"model",cli_version=>"cli_version",
    originator=>"originator",thread_source=>"thread_source",source=>"source",git=>"git",
    info=>"info",usage=>"usage",response_id=>"response_id",rate_limits=>"rate_limits",timestamp=>"timestamp",
});
pub(super) fn string(value: Option<&RawValue>) -> Option<Cow<'_, str>> {
    let raw = value?.get();
    let text = raw.strip_prefix('"')?.strip_suffix('"')?;
    if text.as_bytes().contains(&b'\\') {
        serde_json::from_str::<String>(raw).ok().map(Cow::Owned)
    } else {
        Some(Cow::Borrowed(text))
    }
}
fn raw(
    map: &mut Map<String, Value>,
    key: &str,
    v: Option<&RawValue>,
) -> Result<(), serde_json::Error> {
    if let Some(v) = v {
        map.insert(key.into(), serde_json::from_str(v.get())?);
    }
    Ok(())
}
impl Payload<'_> {
    pub(super) fn kind(&self) -> Option<Cow<'_, str>> {
        string(self.kind)
    }
}
impl Header<'_> {
    pub(super) fn kind(&self) -> Option<Cow<'_, str>> {
        string(self.kind)
    }
    pub(super) fn value(self) -> Result<Value, serde_json::Error> {
        let mut root = Map::new();
        for (key, v) in [
            ("type", self.kind),
            ("timestamp", self.timestamp),
            ("created_at", self.created_at),
            ("createdAt", self.created_at_camel),
            ("uuid", self.uuid),
        ] {
            raw(&mut root, key, v)?;
        }
        if let Some(p) = self.payload {
            let mut value = Map::new();
            for (key, v) in [
                ("type", p.kind),
                ("id", p.id),
                ("cwd", p.cwd),
                ("model", p.model),
                ("cli_version", p.cli_version),
                ("originator", p.originator),
                ("thread_source", p.thread_source),
                ("response_id", p.response_id),
                ("source", p.source),
                ("git", p.git),
                ("info", p.info),
                ("usage", p.usage),
                ("rate_limits", p.rate_limits),
                ("timestamp", p.timestamp),
            ] {
                raw(&mut value, key, v)?;
            }
            root.insert("payload".into(), Value::Object(value));
        }
        Ok(Value::Object(root))
    }
}
