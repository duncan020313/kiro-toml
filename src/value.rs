// TODO: full implementation in task 2
use indexmap::IndexMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UtcOffset {
    Z,
    Minutes(i16),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalDate {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalTime {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub nanosecond: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsetDateTime {
    pub date: LocalDate,
    pub time: LocalTime,
    pub offset: UtcOffset,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalDateTime {
    pub date: LocalDate,
    pub time: LocalTime,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    OffsetDateTime(OffsetDateTime),
    LocalDateTime(LocalDateTime),
    LocalDate(LocalDate),
    LocalTime(LocalTime),
    Array(Vec<Value>),
    Table(IndexMap<String, Value>),
}
