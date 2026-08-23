//! Serde round-trip tests for the domain model.
//!
//! chm-core does not depend on serde_json, so these tests drive serde's data
//! model directly: they serialize into a small in-house JSON tree and
//! deserialize it back through the same impls the cloud API and UI use.

use chm_core::{DataSource, Health, MockDataSource, Overview, QueryRow, TimeRange};
use chrono::{Duration, TimeZone, Utc};
use serde::Serialize;
use serde::de::{
    self, DeserializeOwned, DeserializeSeed, MapAccess, SeqAccess, VariantAccess, Visitor,
};
use serde::ser::{
    self, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant,
    SerializeTupleStruct, SerializeTupleVariant,
};
use std::collections::{BTreeMap, btree_map};
use std::fmt;
use std::task::{Context, Poll};

#[derive(Clone, Debug, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(BTreeMap<String, Json>),
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(map) => map.get(key),
            _ => None,
        }
    }
}

#[derive(Debug)]
struct EncodeError;

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("value cannot be represented as JSON")
    }
}

impl std::error::Error for EncodeError {}
impl ser::Error for EncodeError {
    fn custom<T: fmt::Display>(_: T) -> Self {
        EncodeError
    }
}

#[derive(Debug)]
struct DecodeError(String);

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid JSON input: {}", self.0)
    }
}

impl std::error::Error for DecodeError {}
impl de::Error for DecodeError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        DecodeError(msg.to_string())
    }
}

fn decode_err<T>(what: &str) -> Result<T, DecodeError> {
    Err(DecodeError(what.to_owned()))
}

struct ToJson;

macro_rules! to_num {
    ($($method:ident($ty:ty)),* $(,)?) => {
        $(fn $method(self, v: $ty) -> Result<Json, EncodeError> {
            Ok(Json::Num(v as f64))
        })*
    };
}

impl ser::Serializer for ToJson {
    type Ok = Json;
    type Error = EncodeError;
    type SerializeSeq = SeqToJson;
    type SerializeTuple = SeqToJson;
    type SerializeTupleStruct = SeqToJson;
    type SerializeTupleVariant = TupleVariantToJson;
    type SerializeMap = MapToJson;
    type SerializeStruct = StructToJson;
    type SerializeStructVariant = StructVariantToJson;

    fn serialize_bool(self, v: bool) -> Result<Json, EncodeError> {
        Ok(Json::Bool(v))
    }
    to_num! {
        serialize_i8(i8), serialize_i16(i16), serialize_i32(i32), serialize_i64(i64),
        serialize_u8(u8), serialize_u16(u16), serialize_u32(u32), serialize_u64(u64),
        serialize_f32(f32), serialize_f64(f64),
    }
    fn serialize_char(self, v: char) -> Result<Json, EncodeError> {
        Ok(Json::Str(v.to_string()))
    }
    fn serialize_str(self, v: &str) -> Result<Json, EncodeError> {
        Ok(Json::Str(v.to_owned()))
    }
    fn serialize_bytes(self, _: &[u8]) -> Result<Json, EncodeError> {
        Err(EncodeError)
    }
    fn serialize_none(self) -> Result<Json, EncodeError> {
        Ok(Json::Null)
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Json, EncodeError> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<Json, EncodeError> {
        Ok(Json::Null)
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<Json, EncodeError> {
        Ok(Json::Null)
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> Result<Json, EncodeError> {
        Ok(Json::Str(variant.to_owned()))
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<Json, EncodeError> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Json, EncodeError> {
        Ok(Json::Obj(
            [(variant.to_owned(), value.serialize(ToJson)?)]
                .into_iter()
                .collect(),
        ))
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, EncodeError> {
        Ok(SeqToJson(Vec::with_capacity(len.unwrap_or(0))))
    }
    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, EncodeError> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, EncodeError> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, EncodeError> {
        Ok(TupleVariantToJson {
            variant,
            items: Vec::with_capacity(len),
        })
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, EncodeError> {
        Ok(MapToJson {
            map: BTreeMap::new(),
            pending_key: None,
        })
    }
    fn serialize_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStruct, EncodeError> {
        Ok(StructToJson(BTreeMap::new()))
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, EncodeError> {
        Ok(StructVariantToJson {
            variant,
            fields: BTreeMap::new(),
        })
    }
}

struct SeqToJson(Vec<Json>);

macro_rules! push_element {
    ($vec:expr, $value:expr) => {{
        $vec.push($value.serialize(ToJson)?);
        Ok(())
    }};
}

impl SerializeSeq for SeqToJson {
    type Ok = Json;
    type Error = EncodeError;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), EncodeError> {
        push_element!(self.0, value)
    }
    fn end(self) -> Result<Json, EncodeError> {
        Ok(Json::Arr(self.0))
    }
}

impl ser::SerializeTuple for SeqToJson {
    type Ok = Json;
    type Error = EncodeError;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), EncodeError> {
        push_element!(self.0, value)
    }
    fn end(self) -> Result<Json, EncodeError> {
        Ok(Json::Arr(self.0))
    }
}

impl SerializeTupleStruct for SeqToJson {
    type Ok = Json;
    type Error = EncodeError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), EncodeError> {
        push_element!(self.0, value)
    }
    fn end(self) -> Result<Json, EncodeError> {
        Ok(Json::Arr(self.0))
    }
}

struct TupleVariantToJson {
    variant: &'static str,
    items: Vec<Json>,
}

impl SerializeTupleVariant for TupleVariantToJson {
    type Ok = Json;
    type Error = EncodeError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), EncodeError> {
        push_element!(self.items, value)
    }
    fn end(self) -> Result<Json, EncodeError> {
        Ok(Json::Obj(
            [(self.variant.to_owned(), Json::Arr(self.items))]
                .into_iter()
                .collect(),
        ))
    }
}

struct MapToJson {
    map: BTreeMap<String, Json>,
    pending_key: Option<String>,
}

impl SerializeMap for MapToJson {
    type Ok = Json;
    type Error = EncodeError;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), EncodeError> {
        self.pending_key = Some(match key.serialize(ToJson)? {
            Json::Str(k) => k,
            _ => return Err(EncodeError),
        });
        Ok(())
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), EncodeError> {
        let k = self
            .pending_key
            .take()
            .expect("serialize_value without serialize_key");
        self.map.insert(k, value.serialize(ToJson)?);
        Ok(())
    }
    fn end(self) -> Result<Json, EncodeError> {
        Ok(Json::Obj(self.map))
    }
}

struct StructToJson(BTreeMap<String, Json>);

impl SerializeStruct for StructToJson {
    type Ok = Json;
    type Error = EncodeError;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), EncodeError> {
        self.0.insert(key.to_owned(), value.serialize(ToJson)?);
        Ok(())
    }
    fn end(self) -> Result<Json, EncodeError> {
        Ok(Json::Obj(self.0))
    }
}

struct StructVariantToJson {
    variant: &'static str,
    fields: BTreeMap<String, Json>,
}

impl SerializeStructVariant for StructVariantToJson {
    type Ok = Json;
    type Error = EncodeError;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), EncodeError> {
        self.fields.insert(key.to_owned(), value.serialize(ToJson)?);
        Ok(())
    }
    fn end(self) -> Result<Json, EncodeError> {
        Ok(Json::Obj(
            [(self.variant.to_owned(), Json::Obj(self.fields))]
                .into_iter()
                .collect(),
        ))
    }
}

fn from_json<T: DeserializeOwned>(json: &Json) -> Result<T, DecodeError> {
    T::deserialize(json)
}

#[derive(Clone, Copy)]
struct Ident<'a>(&'a str);

impl<'de> de::Deserializer<'de> for Ident<'de> {
    type Error = DecodeError;

    fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        visitor.visit_borrowed_str(self.0)
    }
    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        visitor.visit_borrowed_str(self.0)
    }
    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        visitor.visit_borrowed_str(self.0)
    }
    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        visitor.visit_borrowed_str(self.0)
    }
    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char bytes byte_buf option unit
        unit_struct newtype_struct seq tuple tuple_struct map struct enum ignored_any
    }
}

struct SliceSeq<'de> {
    iter: std::slice::Iter<'de, Json>,
}

impl<'de> SeqAccess<'de> for SliceSeq<'de> {
    type Error = DecodeError;
    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, DecodeError> {
        match self.iter.next() {
            Some(v) => Ok(Some(seed.deserialize(v)?)),
            None => Ok(None),
        }
    }
}

struct ObjMap<'de> {
    iter: btree_map::Iter<'de, String, Json>,
    pending: Option<&'de Json>,
}

impl<'de> MapAccess<'de> for ObjMap<'de> {
    type Error = DecodeError;
    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, DecodeError> {
        match self.iter.next() {
            Some((k, v)) => {
                self.pending = Some(v);
                Ok(Some(seed.deserialize(Ident(k.as_str()))?))
            }
            None => {
                self.pending = None;
                Ok(None)
            }
        }
    }
    fn next_value_seed<V: DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, DecodeError> {
        seed.deserialize(
            self.pending
                .take()
                .expect("next_value_seed without next_key_seed"),
        )
    }
}

struct UnitVariant<'a> {
    name: &'a str,
}

struct OnlyUnitVariant;

impl<'de> de::EnumAccess<'de> for UnitVariant<'de> {
    type Error = DecodeError;
    type Variant = OnlyUnitVariant;
    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant), DecodeError> {
        Ok((seed.deserialize(Ident(self.name))?, OnlyUnitVariant))
    }
}

impl<'de> VariantAccess<'de> for OnlyUnitVariant {
    type Error = DecodeError;
    fn unit_variant(self) -> Result<(), DecodeError> {
        Ok(())
    }
    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, _: T) -> Result<T::Value, DecodeError> {
        decode_err("expected a unit variant")
    }
    fn tuple_variant<V: Visitor<'de>>(self, _: usize, _: V) -> Result<V::Value, DecodeError> {
        decode_err("expected a unit variant")
    }
    fn struct_variant<V: Visitor<'de>>(
        self,
        _: &'static [&'static str],
        _: V,
    ) -> Result<V::Value, DecodeError> {
        decode_err("expected a unit variant")
    }
}

struct TaggedVariant<'de> {
    tag: Ident<'de>,
    value: &'de Json,
}

impl<'de> de::EnumAccess<'de> for TaggedVariant<'de> {
    type Error = DecodeError;
    type Variant = Self;
    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant), DecodeError> {
        Ok((seed.deserialize(self.tag)?, self))
    }
}

impl<'de> VariantAccess<'de> for TaggedVariant<'de> {
    type Error = DecodeError;
    fn unit_variant(self) -> Result<(), DecodeError> {
        match self.value {
            Json::Null => Ok(()),
            other => decode_err(&format!("expected null for unit variant, got {other:?}")),
        }
    }
    fn newtype_variant_seed<T: DeserializeSeed<'de>>(
        self,
        seed: T,
    ) -> Result<T::Value, DecodeError> {
        seed.deserialize(self.value)
    }
    fn tuple_variant<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, DecodeError> {
        match self.value {
            Json::Arr(_) => de::Deserializer::deserialize_tuple(self.value, len, visitor),
            other => decode_err(&format!("expected tuple variant, got {other:?}")),
        }
    }
    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, DecodeError> {
        match self.value {
            Json::Obj(_) => de::Deserializer::deserialize_struct(self.value, "", fields, visitor),
            other => decode_err(&format!("expected struct variant, got {other:?}")),
        }
    }
}

impl<'de> de::Deserializer<'de> for &'de Json {
    type Error = DecodeError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        match self {
            Json::Null => visitor.visit_unit(),
            Json::Bool(b) => visitor.visit_bool(*b),
            Json::Num(n) => visitor.visit_f64(*n),
            Json::Str(s) => visitor.visit_borrowed_str(s),
            Json::Arr(a) => visitor.visit_seq(SliceSeq { iter: a.iter() }),
            Json::Obj(m) => visitor.visit_map(ObjMap {
                iter: m.iter(),
                pending: None,
            }),
        }
    }
    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        match self {
            Json::Bool(b) => visitor.visit_bool(*b),
            other => decode_err(&format!("expected bool, got {other:?}")),
        }
    }
    fn deserialize_i8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        self.deserialize_i64(visitor)
    }
    fn deserialize_i16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        self.deserialize_i64(visitor)
    }
    fn deserialize_i32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        self.deserialize_i64(visitor)
    }
    fn deserialize_i64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        match self {
            Json::Num(n) => visitor.visit_i64(*n as i64),
            other => decode_err(&format!("expected integer, got {other:?}")),
        }
    }
    fn deserialize_u8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        self.deserialize_u64(visitor)
    }
    fn deserialize_u16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        self.deserialize_u64(visitor)
    }
    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        self.deserialize_u64(visitor)
    }
    fn deserialize_u64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        match self {
            Json::Num(n) => visitor.visit_u64(*n as u64),
            other => decode_err(&format!("expected unsigned integer, got {other:?}")),
        }
    }
    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        self.deserialize_f64(visitor)
    }
    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        match self {
            Json::Num(n) => visitor.visit_f64(*n),
            other => decode_err(&format!("expected number, got {other:?}")),
        }
    }
    fn deserialize_char<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        match self {
            Json::Str(s) if s.chars().count() == 1 => visitor.visit_char(s.chars().next().unwrap()),
            other => decode_err(&format!("expected char, got {other:?}")),
        }
    }
    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        match self {
            Json::Str(s) => visitor.visit_borrowed_str(s),
            other => decode_err(&format!("expected string, got {other:?}")),
        }
    }
    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        self.deserialize_str(visitor)
    }
    fn deserialize_bytes<V: Visitor<'de>>(self, _: V) -> Result<V::Value, DecodeError> {
        decode_err("bytes are not representable")
    }
    fn deserialize_byte_buf<V: Visitor<'de>>(self, _: V) -> Result<V::Value, DecodeError> {
        decode_err("bytes are not representable")
    }
    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        match self {
            Json::Null => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }
    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        match self {
            Json::Null => visitor.visit_unit(),
            other => decode_err(&format!("expected unit, got {other:?}")),
        }
    }
    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        visitor: V,
    ) -> Result<V::Value, DecodeError> {
        self.deserialize_unit(visitor)
    }
    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        visitor: V,
    ) -> Result<V::Value, DecodeError> {
        visitor.visit_newtype_struct(self)
    }
    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        match self {
            Json::Arr(a) => visitor.visit_seq(SliceSeq { iter: a.iter() }),
            other => decode_err(&format!("expected sequence, got {other:?}")),
        }
    }
    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        _: usize,
        visitor: V,
    ) -> Result<V::Value, DecodeError> {
        self.deserialize_seq(visitor)
    }
    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: usize,
        visitor: V,
    ) -> Result<V::Value, DecodeError> {
        self.deserialize_seq(visitor)
    }
    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        match self {
            Json::Obj(m) => visitor.visit_map(ObjMap {
                iter: m.iter(),
                pending: None,
            }),
            other => decode_err(&format!("expected map, got {other:?}")),
        }
    }
    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, DecodeError> {
        self.deserialize_map(visitor)
    }
    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, DecodeError> {
        match self {
            Json::Str(name) => visitor.visit_enum(UnitVariant { name }),
            Json::Obj(m) if m.len() == 1 => {
                let (tag, value) = m.iter().next().unwrap();
                visitor.visit_enum(TaggedVariant {
                    value,
                    tag: Ident(tag.as_str()),
                })
            }
            other => decode_err(&format!("expected enum, got {other:?}")),
        }
    }
    fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        self.deserialize_str(visitor)
    }
    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DecodeError> {
        visitor.visit_unit()
    }
}

fn rt<T>(value: &T) -> T
where
    T: Serialize + DeserializeOwned,
{
    let json = value.serialize(ToJson).expect("encode");
    from_json(&json).expect("decode")
}

fn block_on<F: Future>(fut: F) -> F::Output {
    let fut = std::pin::pin!(fut);
    let waker = std::task::Waker::noop();
    let mut cx = Context::from_waker(waker);
    let mut fut = fut;
    loop {
        if let Poll::Ready(out) = fut.as_mut().poll(&mut cx) {
            return out;
        }
    }
}

#[test]
fn time_range_json_round_trips_each_variant() {
    for range in TimeRange::ALL {
        assert_eq!(rt(&range), range);
    }
    assert_eq!(
        TimeRange::OneHour.serialize(ToJson).unwrap(),
        Json::Str("OneHour".into())
    );
    assert_eq!(
        TimeRange::ThirtyDays.serialize(ToJson).unwrap(),
        Json::Str("ThirtyDays".into())
    );
}

#[test]
fn overview_round_trips_with_all_fields_populated() {
    let overview = Overview {
        qps: 1284.5,
        running_queries: 12,
        slow_queries_24h: 37,
        failed_queries_24h: 3,
        active_merges: 5,
        replicas_ok: 3,
        replicas_total: 3,
        tables_total: 142,
        parts_total: 8931,
        disk_used_bytes: 512 * 1024 * 1024 * 1024,
        disk_total_bytes: 1024_u64 * 1024 * 1024 * 1024,
        uptime_seconds: 86_400 * 12,
        clickhouse_version: "25.3.1.1 (smoke)".into(),
    };
    assert_eq!(rt(&overview), overview);
}

#[test]
fn query_row_round_trips_with_and_without_optional_fields() {
    let started =
        Utc.with_ymd_and_hms(2026, 2, 14, 9, 30, 0).unwrap() + Duration::milliseconds(250);
    let failed_row = QueryRow {
        id: "q-5".into(),
        user: "svc-ingest".into(),
        elapsed_ms: 210.5,
        memory_bytes: 4 << 30,
        read_rows: 123_456_789,
        read_bytes: 456_789_123,
        exception: Some("Table is readonly (replica delay)".into()),
        normalized_sql: "INSERT INTO ingest.events VALUES (…)".into(),
        started_at: Some(started),
    };
    assert_eq!(rt(&failed_row), failed_row);

    let running_row = QueryRow {
        id: "q-1".into(),
        user: "analytics".into(),
        elapsed_ms: 12_450.0,
        memory_bytes: 4 << 30,
        read_rows: 123_456_789,
        read_bytes: 456_789_123,
        exception: None,
        normalized_sql: "SELECT count() FROM events GROUP BY user_id HAVING …".into(),
        started_at: None,
    };
    assert_eq!(rt(&running_row), running_row);

    let json = failed_row.serialize(ToJson).unwrap();
    assert_eq!(
        json.get("exception"),
        Some(&Json::Str("Table is readonly (replica delay)".into()))
    );
    assert_eq!(
        running_row.serialize(ToJson).unwrap().get("exception"),
        Some(&Json::Null)
    );
}

#[test]
fn health_round_trips() {
    let health = Health {
        ok: true,
        readonly_tables: 0,
        replication_lag_max_sec: 1.5,
        zookeeper_available: true,
        delayed_inserts: 7,
        distributed_files_to_insert: 9,
        background_pool_utilization: 0.34,
    };
    assert_eq!(rt(&health), health);
}

#[test]
fn mock_fixtures_survive_round_trip() {
    let ds = MockDataSource::new("roundtrip");

    let overview = block_on(ds.overview(TimeRange::SevenDays)).unwrap();
    assert_eq!(rt(&overview), overview);

    let health = block_on(ds.health()).unwrap();
    assert_eq!(rt(&health), health);

    let failed = block_on(ds.failed_queries(TimeRange::TwentyFourHours)).unwrap();
    assert!(!failed.is_empty());
    for row in &failed {
        assert_eq!(rt(row), *row);
    }
}
