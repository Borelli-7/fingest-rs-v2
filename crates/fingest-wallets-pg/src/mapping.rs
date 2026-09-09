use bigdecimal::BigDecimal;
use chrono::NaiveDate;
use fingest_kernel::{CategoryRef, Currency, Money, PortError};
use fingest_wallets_core::{Expense, Wallet};

/// Rebuilds a `Money` from its two stored columns.
pub(crate) fn money(amount: BigDecimal, currency: String) -> Result<Money, PortError> {
    let currency = Currency::new(&currency)
        .map_err(|e| PortError::Storage(format!("stored currency is invalid: {e}")))?;
    Ok(Money::new(amount, currency))
}

/// Built field-by-field rather than through `Wallet::new`: the row is already persisted, and
/// tightening a domain rule must not make existing rows unreadable.
pub(crate) fn wallet(
    id: i32,
    name: String,
    amount: BigDecimal,
    currency: String,
) -> Result<Wallet, PortError> {
    Ok(Wallet {
        id: Some(id),
        name,
        amount: money(amount, currency)?,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn expense(
    id: i32,
    amount: BigDecimal,
    currency: String,
    date: NaiveDate,
    description: String,
    category_name: String,
    category_profit: bool,
) -> Result<Expense, PortError> {
    Ok(Expense {
        id: Some(id),
        amount: money(amount, currency)?,
        date,
        description,
        category: CategoryRef {
            name: category_name,
            profit: category_profit,
        },
    })
}

pub(crate) fn to_port_error(err: sqlx::Error) -> PortError {
    if let sqlx::Error::Database(ref db_err) = err {
        match db_err.code().as_deref() {
            Some("23505") => return PortError::Conflict("Record already exists".to_owned()),
            Some("23503") => {
                return PortError::Conflict(
                    "Referenced record does not exist or is still in use".to_owned(),
                );
            }
            _ => {}
        }
    }
    PortError::Storage(err.to_string())
}
