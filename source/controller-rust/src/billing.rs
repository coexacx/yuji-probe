use crate::{core::*, model::*};
use chrono::{DateTime, Datelike, Duration, Months, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenewalCycle {
    pub unit: String,
    pub count: u32,
}
impl Default for RenewalCycle {
    fn default() -> Self {
        Self {
            unit: "days".into(),
            count: 30,
        }
    }
}
impl RenewalCycle {
    pub fn valid(&self) -> bool {
        match self.unit.as_str() {
            "months" => (1..=120).contains(&self.count),
            "days" => (1..=3650).contains(&self.count),
            _ => false,
        }
    }
    pub fn label(&self) -> String {
        match (self.unit.as_str(), self.count) {
            ("months", 1) => "每月".into(),
            ("months", 3) => "每季度".into(),
            ("months", 12) => "每年".into(),
            ("months", n) => format!("每 {n} 个月"),
            (_, n) => format!("每 {n} 天"),
        }
    }
}
// Amounts are decimal strings at the API boundary and integers in the currency's
// minor unit for calculation. Missing amounts never imply a free server.
pub fn precision(currency: &str) -> Option<u32> {
    match currency {
        "JPY" | "KRW" | "VND" | "CLP" | "ISK" => Some(0),
        "BHD" | "KWD" | "OMR" | "JOD" | "TND" => Some(3),
        "CNY" | "USD" | "EUR" | "GBP" | "HKD" | "TWD" | "SGD" | "AUD" | "CAD" | "NZD" | "CHF"
        | "SEK" | "NOK" | "DKK" | "PLN" | "CZK" | "HUF" | "RON" | "RUB" | "UAH" | "TRY" | "INR"
        | "IDR" | "MYR" | "THB" | "PHP" | "BRL" | "MXN" | "ZAR" | "AED" | "SAR" | "ILS" | "PKR"
        | "BDT" | "ARS" => Some(2),
        _ => None,
    }
}
pub fn minor(amount: &str, currency: &str) -> Option<u64> {
    let digits = precision(currency)? as usize;
    let (whole, fraction) = amount.split_once('.').unwrap_or((amount, ""));
    if whole.is_empty()
        || whole.len() > 9
        || !whole.bytes().all(|c| c.is_ascii_digit())
        || !fraction.bytes().all(|c| c.is_ascii_digit())
        || fraction.len() > digits
        || (amount.contains('.') && fraction.is_empty())
    {
        return None;
    }
    let scale = 10u64.pow(digits as u32);
    Some(
        whole.parse::<u64>().ok()? * scale
            + if fraction.is_empty() {
                0
            } else {
                fraction.parse::<u64>().ok()? * 10u64.pow((digits - fraction.len()) as u32)
            },
    )
}
pub fn decimal(value: u64, currency: &str) -> String {
    let p = precision(currency).unwrap_or(2);
    if p == 0 {
        return value.to_string();
    }
    let scale = 10u64.pow(p);
    format!(
        "{}.{:0width$}",
        value / scale,
        value % scale,
        width = p as usize
    )
}
pub fn amount_label(amount: &str, currency: &str) -> String {
    minor(amount, currency)
        .map(|n| format!("{currency} {}", decimal(n, currency)))
        .unwrap_or_else(|| "金额未设置".into())
}
pub fn next_expiry(node: &Node) -> ApiResult<String> {
    let cycle = &node.renewal_cycle;
    if !cycle.valid() {
        return Err(ApiError::new(400, "续费周期不正确"));
    }
    let current = DateTime::parse_from_rfc3339(&node.expires_at)
        .map_err(|_| ApiError::new(400, "到期时间不正确"))?
        .with_timezone(&crate::nodes::zone());
    let next = if cycle.unit == "months" {
        let anchor = if (1..=31).contains(&node.renewal_anchor_day) {
            node.renewal_anchor_day
        } else {
            current.day()
        };
        let first = current
            .with_day(1)
            .and_then(|d| d.checked_add_months(Months::new(cycle.count)))
            .ok_or_else(|| ApiError::new(400, "到期时间超出范围"))?;
        (1..=anchor).rev().find_map(|day| first.with_day(day))
    } else {
        current.checked_add_signed(Duration::days(i64::from(cycle.count)))
    }
    .filter(|d| (2000..=2199).contains(&d.year()))
    .ok_or_else(|| ApiError::new(400, "请在服务器设置中调整到期时间"))?;
    Ok(next
        .with_timezone(&Utc)
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}
pub fn date_anchor(value: &str) -> u32 {
    DateTime::parse_from_rfc3339(value)
        .map(|d| d.with_timezone(&crate::nodes::zone()).day())
        .unwrap_or(0)
}
pub fn summary(nodes: &[Node], at: i64) -> Value {
    let mut rows: BTreeMap<String, (u64, u64, u64, usize)> = BTreeMap::new();
    let mut unpriced = 0;
    for n in nodes
        .iter()
        .filter(|n| !n.demo && !n.removing && n.notify_renewal && !n.expires_at.is_empty())
    {
        let Some(cost) = minor(&n.renewal_amount, &n.renewal_currency) else {
            unpriced += 1;
            continue;
        };
        if !n.renewal_cycle.valid() {
            unpriced += 1;
            continue;
        }
        let entry = rows.entry(n.renewal_currency.clone()).or_default();
        let factor = if n.renewal_cycle.unit == "months" {
            12
        } else {
            365
        };
        let count = u64::from(n.renewal_cycle.count);
        entry.0 += (cost * factor + count / 2) / count;
        if let Ok(d) = DateTime::parse_from_rfc3339(&n.expires_at) {
            if d.timestamp() < at {
                entry.2 += cost;
            } else if d.timestamp() < at + 30 * 86400 {
                entry.1 += cost;
            }
        }
        entry.3 += 1;
    }
    let rows: Vec<_> = rows
        .into_iter()
        .map(|(currency, (annual, upcoming, overdue, count))| {
            json!({"currency":currency,"monthly":decimal((annual+6)/12,&currency),
            "yearly":decimal(annual,&currency),"next30Days":decimal(upcoming,&currency),
            "overdue":decimal(overdue,&currency),"nodes":count})
        })
        .collect();
    json!({"currencies":rows,"unpriced":unpriced})
}
#[cfg(test)]
mod tests {
    use super::*;
    fn node(date: &str, unit: &str, count: u32) -> Node {
        Node {
            expires_at: date.into(),
            renewal_cycle: RenewalCycle {
                unit: unit.into(),
                count,
            },
            ..Default::default()
        }
    }
    #[test]
    fn calendar_months_keep_original_day_and_local_time() {
        let mut n = node("2027-01-31T02:30:00Z", "months", 1);
        n.renewal_anchor_day = 31;
        assert_eq!(next_expiry(&n).unwrap(), "2027-02-28T02:30:00Z");
        n.expires_at = next_expiry(&n).unwrap();
        assert_eq!(next_expiry(&n).unwrap(), "2027-03-31T02:30:00Z");
        let n = node("2024-02-29T02:30:00Z", "months", 12);
        assert_eq!(next_expiry(&n).unwrap(), "2025-02-28T02:30:00Z");
        let n = node("2026-01-30T20:00:00Z", "months", 1);
        assert_eq!(next_expiry(&n).unwrap(), "2026-02-27T20:00:00Z");
    }
    #[test]
    fn days_ranges_and_old_records() {
        assert_eq!(
            next_expiry(&node("2026-12-30T10:00:00Z", "days", 7)).unwrap(),
            "2027-01-06T10:00:00Z"
        );
        for (unit, count) in [
            ("days", 0),
            ("days", 3651),
            ("months", 0),
            ("months", 121),
            ("years", 1),
        ] {
            assert!(next_expiry(&node("2026-01-01T00:00:00Z", unit, count)).is_err());
        }
        assert!(next_expiry(&node("2199-12-31T00:00:00Z", "months", 1)).is_err());
        let old: Node = serde_json::from_str(r#"{"expiresAt":"2026-01-01T00:00:00Z"}"#).unwrap();
        assert_eq!(old.renewal_cycle, RenewalCycle::default());
        assert!(old.renewal_amount.is_empty());
    }
    #[test]
    fn decimal_money_never_accepts_negative_float_or_unknown_currency() {
        assert_eq!(minor("12.30", "CNY"), Some(1230));
        assert_eq!(minor("0", "CNY"), Some(0));
        assert_eq!(minor("999999999.99", "USD"), Some(99999999999));
        assert_eq!(minor("100", "JPY"), Some(100));
        assert_eq!(minor("1.234", "KWD"), Some(1234));
        for (a, c) in [
            ("-1", "USD"),
            ("1e2", "USD"),
            ("1.234", "USD"),
            ("1.2", "JPY"),
            ("NaN", "CNY"),
            ("", "USD"),
            (".1", "USD"),
            ("1.", "USD"),
            ("1000000000", "USD"),
            ("1", "BAD"),
        ] {
            assert!(minor(a, c).is_none(), "{a} {c}");
        }
    }
    #[test]
    fn estimate_separates_currency_stopped_nodes_and_unknown_amounts() {
        let mut n = node("2026-10-10T00:00:00Z", "months", 3);
        n.notify_renewal = true;
        n.renewal_amount = "30.00".into();
        n.renewal_currency = "USD".into();
        let mut other = n.clone();
        other.renewal_currency = "CNY".into();
        let mut unknown = n.clone();
        unknown.renewal_amount.clear();
        let mut stopped = n.clone();
        stopped.notify_renewal = false;
        let mut overdue = n.clone();
        overdue.expires_at = "2026-09-01T00:00:00Z".into();
        let at = DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z")
            .unwrap()
            .timestamp();
        let v = summary(&[n, other, unknown, stopped, overdue], at);
        assert_eq!(v["unpriced"], 1);
        assert_eq!(v["currencies"][0]["currency"], "CNY");
        assert_eq!(v["currencies"][0]["monthly"], "10.00");
        assert_eq!(v["currencies"][1]["yearly"], "240.00");
        assert_eq!(v["currencies"][1]["next30Days"], "30.00");
        assert_eq!(v["currencies"][1]["overdue"], "30.00");
    }
}
