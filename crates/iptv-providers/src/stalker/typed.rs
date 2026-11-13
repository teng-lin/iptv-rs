//! Typed page deserialization: rows become domain records as serde traverses
//! the page. Unknown data is discarded, never retained as a JSON value tree.
use super::Page;
use crate::{
    HttpResponse, ProviderError,
    model::{Import, WarningLocation},
    typed_row::{Row, Scalar},
};
use serde::{
    Deserializer,
    de::{self, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor},
};
use std::{fmt, marker::PhantomData};

type MapRow<T> = fn(&Row) -> Result<T, &'static str>;
struct RowSeed<T>(MapRow<T>);
impl<'de, T> DeserializeSeed<'de> for RowSeed<T> {
    type Value = Result<T, &'static str>;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        struct V<T>(MapRow<T>);
        impl<'de, T> Visitor<'de> for V<T> {
            type Value = Result<T, &'static str>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a catalog row")
            }
            fn visit_map<A: MapAccess<'de>>(self, a: A) -> Result<Self::Value, A::Error> {
                let row = <Row as serde::Deserialize>::deserialize(
                    de::value::MapAccessDeserializer::new(a),
                )?;
                Ok((self.0)(&row))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                while a.next_element::<IgnoredAny>()?.is_some() {}
                Ok(Err("row is not an object"))
            }
            fn visit_str<E: de::Error>(self, _: &str) -> Result<Self::Value, E> {
                Ok(Err("row is not an object"))
            }
            fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> {
                Ok(Err("row is not an object"))
            }
            fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self::Value, E> {
                Ok(Err("row is not an object"))
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
                Ok(Err("row is not an object"))
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> {
                Ok(Err("row is not an object"))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(Err("row is not an object"))
            }
        }
        d.deserialize_any(V(self.0))
    }
}
struct Rows<T>(MapRow<T>);
fn add<T, E: de::Error>(
    out: &mut Import<T>,
    row: Result<T, &'static str>,
    index: usize,
) -> Result<(), E> {
    if index as u64 >= iptv_core::import_limits::ImportLimits::default().items {
        return Err(E::custom("item budget exceeded"));
    }
    match row {
        Ok(row) => out.items.push(row),
        Err(why) => out.skip(WarningLocation::Index(index), why),
    }
    Ok(())
}
impl<'de, T> Visitor<'de> for Rows<T> {
    type Value = Import<T>;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a list of catalog rows")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
        let mut out = Import::default();
        let mut index = 0;
        while let Some(row) = a.next_element_seed(RowSeed(self.0))? {
            add(&mut out, row, index)?;
            index += 1;
        }
        Ok(out)
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
        let mut out = Import::default();
        let mut index = 0;
        while a.next_key::<IgnoredAny>()?.is_some() {
            let row = a.next_value_seed(RowSeed(self.0))?;
            add(&mut out, row, index)?;
            index += 1;
        }
        Ok(out)
    }
}
impl<'de, T> DeserializeSeed<'de> for Rows<T> {
    type Value = Import<T>;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        d.deserialize_any(self)
    }
}
struct Payload<T>(MapRow<T>);
impl<'de, T> DeserializeSeed<'de> for Payload<T> {
    type Value = Page<T>;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        struct V<T>(MapRow<T>);
        impl<'de, T> Visitor<'de> for V<T> {
            type Value = Page<T>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a portal page")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, a: A) -> Result<Self::Value, A::Error> {
                Ok(Page {
                    items: Rows(self.0).visit_seq(a)?,
                    total_items: None,
                    max_page_items: None,
                    selected_item: None,
                })
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                let mut rows = None;
                let (mut total, mut maximum, mut selected) = (None, None, None);
                while let Some(key) = a.next_key::<String>()? {
                    match key.as_str() {
                        "data" => {
                            if rows.is_some() {
                                return Err(de::Error::custom("duplicate data"));
                            }
                            rows = Some(a.next_value_seed(Rows(self.0))?);
                        }
                        "total_items" | "max_page_items" | "selected_item" => {
                            let scalar = a.next_value::<Scalar>()?;
                            let n = scalar.string().and_then(|s| s.parse::<u32>().ok());
                            match key.as_str() {
                                "total_items" => total = n,
                                "max_page_items" => maximum = n.filter(|n| *n > 0),
                                _ => selected = n,
                            }
                        }
                        _ => {
                            a.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                Ok(Page {
                    items: rows.ok_or_else(|| de::Error::custom("missing data"))?,
                    total_items: total,
                    max_page_items: maximum,
                    selected_item: selected,
                })
            }
        }
        d.deserialize_any(V(self.0))
    }
}
struct Envelope<T>(MapRow<T>, PhantomData<T>);
impl<'de, T> DeserializeSeed<'de> for Envelope<T> {
    type Value = Page<T>;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        struct V<T>(MapRow<T>);
        impl<'de, T> Visitor<'de> for V<T> {
            type Value = Page<T>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a portal envelope")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                let mut page = None;
                while let Some(key) = a.next_key::<String>()? {
                    if key == "js" {
                        if page.is_some() {
                            return Err(de::Error::custom("duplicate js"));
                        }
                        page = Some(a.next_value_seed(Payload(self.0))?);
                    } else {
                        a.next_value::<IgnoredAny>()?;
                    }
                }
                page.ok_or_else(|| de::Error::custom("missing js"))
            }
        }
        d.deserialize_map(V(self.0))
    }
}
pub(super) fn page<T>(response: &HttpResponse, map: MapRow<T>) -> Result<Page<T>, ProviderError> {
    if !response.is_success() {
        return Err(ProviderError::HttpStatus(response.status));
    }
    if let Some(f) = super::classify_auth_failure(response) {
        return Err(f.into_error().into());
    }
    let probe = super::auth_probe(response);
    if let Some(error) = probe.error.filter(|s| !s.trim().is_empty()) {
        return Err(crate::StalkerError::with_text(
            crate::StalkerErrorKind::Portal,
            Some(super::strip_markup(&error)),
        )
        .into());
    }
    let bytes = response
        .body
        .strip_prefix(b"\xEF\xBB\xBF")
        .unwrap_or(&response.body);
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let out = Envelope(map, PhantomData)
        .deserialize(&mut deserializer)
        .map_err(|_| ProviderError::InvalidBody("invalid portal page".into()))?;
    deserializer
        .end()
        .map_err(|_| ProviderError::InvalidBody("trailing portal page data".into()))?;
    Ok(out)
}
