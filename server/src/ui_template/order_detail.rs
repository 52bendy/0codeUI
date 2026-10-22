//! `ORDER_DETAIL_PAGE` 场景：业务数据校验与模板视图模型构建。
//!
//! 业务数据契约与 PRD §5.1 对齐：
//! `{ order_id, status, items: [{ name, price, qty }] }`。

use rust_decimal::Decimal;
use serde_json::{json, Value};

use crate::models::OrderBusinessData;
use crate::ui_template::{DeviceInfo, TemplateError};

/// 合法订单状态（与数据底座种子 / PRD 状态机一致）。
const VALID_STATUSES: &[&str] = &["PENDING", "PAID", "SHIPPED", "COMPLETED", "CANCELLED"];

/// 解析并校验业务数据，产出可直接填充模板的视图模型。
pub fn build_view(business_data: &Value, device: &DeviceInfo) -> Result<Value, TemplateError> {
    let data: OrderBusinessData =
        serde_json::from_value(business_data.clone()).map_err(|error| {
            TemplateError::InvalidBusinessData(format!(
                "does not match ORDER_DETAIL_PAGE contract: {error}"
            ))
        })?;

    validate(&data)?;

    let total = data
        .items
        .iter()
        .map(|item| item.price * Decimal::from(item.qty))
        .sum::<Decimal>();
    let total_qty: i32 = data.items.iter().map(|item| item.qty).sum();
    let (status_label, status_class) = status_view(&data.status);

    let items: Vec<Value> = data
        .items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            json!({
                "index": index + 1,
                "name": item.name,
                "price": format_money(item.price),
                "qty": item.qty,
                "amount": format_money(item.price * Decimal::from(item.qty)),
            })
        })
        .collect();

    Ok(json!({
        "order_id": data.order_id,
        "status": data.status,
        "status_label": status_label,
        "status_class": status_class,
        "item_count": data.items.len(),
        "total_qty": total_qty,
        "total": format_money(total),
        "currency_symbol": "¥",
        "refresh_url": format!("/api/v1/orders/{}/business-data", data.order_id),
        "device_class": device_class(device),
        "items": items,
    }))
}

fn validate(data: &OrderBusinessData) -> Result<(), TemplateError> {
    if data.order_id.is_empty() || data.order_id.chars().count() > 64 {
        return Err(TemplateError::InvalidBusinessData(
            "order_id must be 1..=64 characters".to_string(),
        ));
    }
    if !data
        .order_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err(TemplateError::InvalidBusinessData(
            "order_id may only contain ASCII letters, digits, `_` and `-`".to_string(),
        ));
    }
    if !VALID_STATUSES.contains(&data.status.as_str()) {
        return Err(TemplateError::InvalidBusinessData(format!(
            "status must be one of {}",
            VALID_STATUSES.join(", ")
        )));
    }
    if data.items.is_empty() {
        return Err(TemplateError::InvalidBusinessData(
            "items must contain at least 1 item".to_string(),
        ));
    }
    if data.items.len() > 100 {
        return Err(TemplateError::InvalidBusinessData(
            "items must contain at most 100 items".to_string(),
        ));
    }

    for (index, item) in data.items.iter().enumerate() {
        if item.name.trim().is_empty() || item.name.chars().count() > 200 {
            return Err(TemplateError::InvalidBusinessData(format!(
                "items[{index}].name must be 1..=200 characters"
            )));
        }
        if item.price < Decimal::ZERO || item.price > Decimal::from(1_000_000_000_i64) {
            return Err(TemplateError::InvalidBusinessData(format!(
                "items[{index}].price must be within 0..=1000000000"
            )));
        }
        if item.qty < 1 || item.qty > 9999 {
            return Err(TemplateError::InvalidBusinessData(format!(
                "items[{index}].qty must be within 1..=9999"
            )));
        }
    }
    Ok(())
}

fn status_view(status: &str) -> (&'static str, &'static str) {
    match status {
        "PENDING" => ("待支付", "status-pending"),
        "PAID" => ("已支付", "status-paid"),
        "SHIPPED" => ("已发货", "status-shipped"),
        "COMPLETED" => ("已完成", "status-completed"),
        "CANCELLED" => ("已取消", "status-cancelled"),
        _ => ("未知状态", "status-unknown"),
    }
}

fn format_money(amount: Decimal) -> String {
    format!("{amount:.2}")
}

fn device_class(device: &DeviceInfo) -> String {
    let platform = match device.platform.trim().to_ascii_lowercase().as_str() {
        "android" => "android",
        "ios" => "ios",
        "harmonyos" | "harmony" => "harmonyos",
        _ => "web",
    };
    let width = match device.screen_width {
        0..=479 => "compact",
        480..=767 => "regular",
        _ => "wide",
    };
    format!("platform-{platform} width-{width}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_is_formatted_with_two_decimals() {
        assert_eq!(format_money(Decimal::new(29900, 2)), "299.00");
        assert_eq!(format_money(Decimal::new(0, 2)), "0.00");
    }

    #[test]
    fn device_class_buckets_width() {
        let compact = DeviceInfo {
            screen_width: 360,
            ..DeviceInfo::default()
        };
        assert_eq!(device_class(&compact), "platform-web width-compact");
        let wide = DeviceInfo {
            screen_width: 800,
            ..DeviceInfo::default()
        };
        assert_eq!(device_class(&wide), "platform-web width-wide");
    }
}
