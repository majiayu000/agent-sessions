use crate::LineErrorKind;
use jiff::Timestamp;
use serde_json::Value;

pub(crate) fn seconds(v: Option<&Value>) -> Result<Option<Timestamp>, LineErrorKind> {
    let Some(v) = v.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let seconds = v
        .as_f64()
        .ok_or_else(|| LineErrorKind::InvalidField("timestamp".into()))?;
    let whole = seconds.trunc();
    if !seconds.is_finite() || whole < i64::MIN as f64 || whole >= i64::MAX as f64 {
        return Err(LineErrorKind::InvalidField("timestamp".into()));
    }

    // Split the value before scaling to retain fractional precision at large
    // epoch values and avoid rounding a small negative fraction up to a second.
    let fractional_nanos = (seconds.fract() * 1e9).floor() as i128;
    Timestamp::from_nanosecond(whole as i128 * 1_000_000_000 + fractional_nanos)
        .map(Some)
        .map_err(|_| LineErrorKind::InvalidField("timestamp".into()))
}

pub(crate) fn millis(v: Option<&Value>) -> Result<Option<Timestamp>, LineErrorKind> {
    match v.filter(|v| !v.is_null()) {
        None => Ok(None),
        Some(v) => v
            .as_i64()
            .and_then(|value| Timestamp::from_millisecond(value).ok())
            .map(Some)
            .ok_or_else(|| LineErrorKind::InvalidField("timestamp".into())),
    }
}
