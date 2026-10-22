//! 核心业务领域模型（REQ-202610-1170）。
//!
//! 金额字段统一使用 [`rust_decimal::Decimal`]，序列化为 JSON 数字（保留十进制精度），
//! 不使用任何浮点类型。SQLite 侧以「分」为单位的整数存储，见 [`cents_to_decimal`]。

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// 金额换算错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MoneyError {
    /// 金额包含超过两位的小数（无法精确换算为分）。
    #[error("amount must have at most 2 decimal places")]
    TooManyDecimalPlaces,
    /// 金额超出 i64 可表示范围。
    #[error("amount is out of range")]
    OutOfRange,
}

/// 「分」转 [`Decimal`]：`29900` → `299.00`。
pub fn cents_to_decimal(cents: i64) -> Decimal {
    Decimal::new(cents, 2)
}

/// [`Decimal`] 转「分」：`299.00` → `29900`；超过两位小数或溢出时报错。
pub fn decimal_to_cents(amount: Decimal) -> Result<i64, MoneyError> {
    let scaled = amount * Decimal::from(100);
    if !scaled.fract().is_zero() {
        return Err(MoneyError::TooManyDecimalPlaces);
    }
    scaled.to_i64().ok_or(MoneyError::OutOfRange)
}

/// 用户。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub display_name: String,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub balance: Decimal,
    pub currency: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 用户余额（`query_user_balance` 原子能力的返回结构）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserBalance {
    pub user_id: String,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub balance: Decimal,
    pub currency: String,
}

/// 商品。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Product {
    pub id: String,
    pub name: String,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub price: Decimal,
    pub currency: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 订单。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Order {
    pub id: String,
    pub user_id: String,
    pub status: String,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub total_amount: Decimal,
    pub currency: String,
    pub shipping_address: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 订单项（商品名称与单价为下单时快照）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderItem {
    pub id: String,
    pub order_id: String,
    pub product_id: Option<String>,
    pub name: String,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub unit_price: Decimal,
    pub qty: i32,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub amount: Decimal,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 订单业务数据：与 PRD §5.1 `business_data` 完全同构。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderBusinessData {
    pub order_id: String,
    pub status: String,
    pub items: Vec<OrderBusinessItem>,
}

/// PRD §5.1 `business_data.items[]` 单项。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderBusinessItem {
    pub name: String,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub price: Decimal,
    pub qty: i32,
}

/// 操作记录（原子能力审计）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationRecord {
    pub id: String,
    pub actor_user_id: Option<String>,
    pub capability: String,
    pub entity_type: String,
    pub entity_id: String,
    pub request_payload: String,
    pub result_payload: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

/// 创建订单入参。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateOrderInput {
    /// 调用方用户。
    pub user_id: String,
    /// 可选的指定订单号；缺省时自动生成。
    pub order_id: Option<String>,
    /// 收货地址。
    pub shipping_address: Option<String>,
    /// 订单项（商品 + 数量）。
    pub items: Vec<CreateOrderItemInput>,
}

/// 创建订单的商品行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateOrderItemInput {
    pub product_id: String,
    pub qty: i32,
}

/// `delete_record` 原子能力的删除结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeletedRecord {
    pub entity_type: String,
    pub entity_id: String,
    pub deleted: bool,
}

/// 种子数据加载统计（均为本次实际插入行数；重复加载为 0）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeedReport {
    pub users: usize,
    pub products: usize,
    pub orders: usize,
    pub order_items: usize,
    pub operation_records: usize,
}

impl SeedReport {
    /// 本次加载的总行数。
    pub const fn total(&self) -> usize {
        self.users + self.products + self.orders + self.order_items + self.operation_records
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cents_and_decimal_round_trip() {
        assert_eq!(cents_to_decimal(29900), Decimal::new(29900, 2));
        assert_eq!(decimal_to_cents(Decimal::new(29900, 2)).unwrap(), 29900);
        assert_eq!(
            decimal_to_cents(Decimal::from_str_exact("0.01").unwrap()).unwrap(),
            1
        );
    }

    #[test]
    fn decimal_to_cents_rejects_sub_cent_amounts() {
        let amount = Decimal::from_str_exact("1.005").unwrap();
        assert_eq!(
            decimal_to_cents(amount),
            Err(MoneyError::TooManyDecimalPlaces)
        );
    }

    #[test]
    fn money_arithmetic_is_exact() {
        let a = Decimal::from_str_exact("0.10").unwrap();
        let b = Decimal::from_str_exact("0.20").unwrap();
        assert_eq!((a + b).to_string(), "0.30");
    }
}
