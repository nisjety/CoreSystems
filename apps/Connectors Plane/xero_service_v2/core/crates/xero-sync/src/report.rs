//! Pure `Reports/* → fetch params` mapping for the `Reports { as_of }` mode.
//!
//! Side-effect free (no I/O, no implicit `Utc::now()`): the caller supplies the
//! `as_of` date, so the period derivation and the object-key `period_key` are
//! fully unit-testable.

use chrono::{Datelike, NaiveDate};
use uuid::Uuid;
use xero_common::EntityType;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReportOptions {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub periods: Option<u8>,
}

impl ReportOptions {
    pub fn validate(&self, entities: &[EntityType]) -> Result<(), String> {
        if self.from.is_some() != self.to.is_some() {
            return Err("report-from and report-to must be supplied together".into());
        }
        if self.from.zip(self.to).is_some_and(|(from, to)| from > to) {
            return Err("report-from must be on or before report-to".into());
        }
        if self
            .periods
            .is_some_and(|periods| !(1..=12).contains(&periods))
        {
            return Err("report-periods must be between 1 and 12".into());
        }
        if self.from.is_some()
            && entities.iter().any(|entity| {
                !matches!(
                    entity,
                    EntityType::ReportProfitAndLoss | EntityType::ReportBankSummary
                )
            })
        {
            return Err("explicit report dates require only ProfitAndLoss or BankSummary".into());
        }
        if self.periods.is_some()
            && entities.iter().any(|entity| {
                !matches!(
                    entity,
                    EntityType::ReportProfitAndLoss | EntityType::ReportBalanceSheet
                )
            })
        {
            return Err("report-periods requires only ProfitAndLoss or BalanceSheet".into());
        }
        Ok(())
    }
}

/// The resolved per-report fetch + metadata spec produced by [`report_params`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportSpec {
    /// `extra` query overrides forwarded to `fetch_report_raw` (date params).
    pub extra: Vec<(String, String)>,
    /// Filename business-period key (e.g. `asof-2026-06-17`).
    pub period_key: String,
    /// `x-report-date` metadata (as-of reports).
    pub report_date: Option<String>,
    /// `x-report-from` metadata (period reports).
    pub report_from: Option<String>,
    /// `x-report-to` metadata (period reports).
    pub report_to: Option<String>,
}

impl ReportSpec {
    /// Full sorted param signature for `x-report-params` (reproducible key).
    pub fn params_signature(&self) -> String {
        let mut pairs: Vec<String> = self.extra.iter().map(|(k, v)| format!("{k}={v}")).collect();
        pairs.sort();
        pairs.join("&")
    }
}

/// Derive a report's `period_key` (used in the object filename) + the
/// `extra` query overrides that pin the report to `as_of`.
///
/// `as-of` reports get `date=<as_of>` and `period_key = asof-<as_of>`.
/// `period` reports (P&L / BankSummary) get `fromDate=<month start>` and
/// `toDate=<as_of>` (month-to-date) and `period_key = <from>_<to>`.
pub fn report_params(entity: &EntityType, as_of: NaiveDate) -> ReportSpec {
    let as_of_str = as_of.format("%Y-%m-%d").to_string();
    match entity {
        EntityType::ReportProfitAndLoss | EntityType::ReportBankSummary => {
            let from = as_of.with_day(1).unwrap_or(as_of);
            let from_str = from.format("%Y-%m-%d").to_string();
            ReportSpec {
                extra: vec![
                    ("fromDate".to_owned(), from_str.clone()),
                    ("toDate".to_owned(), as_of_str.clone()),
                ],
                period_key: format!("{from_str}_{as_of_str}"),
                report_date: None,
                report_from: Some(from_str),
                report_to: Some(as_of_str),
            }
        }
        _ => ReportSpec {
            extra: vec![("date".to_owned(), as_of_str.clone())],
            period_key: format!("asof-{as_of_str}"),
            report_date: Some(as_of_str),
            report_from: None,
            report_to: None,
        },
    }
}

/// Derive the strictly contact-scoped parameters for Xero's aged receivables
/// and payables reports.
///
/// These endpoints are intentionally excluded from the generic report fan-out:
/// a contact UUID belongs to exactly one tenant and must be discovered within
/// that tenant before the report is requested. The contact qualifier prevents
/// two reports from the same run and as-of date sharing a raw object key.
pub fn aged_contact_report_params(
    entity: &EntityType,
    as_of: NaiveDate,
    contact_id: Uuid,
) -> Result<ReportSpec, String> {
    if !matches!(
        entity,
        EntityType::ReportAgedReceivablesByContact | EntityType::ReportAgedPayablesByContact
    ) {
        return Err(format!(
            "{entity} is not an aged-by-contact report; expected ReportAgedReceivablesByContact or ReportAgedPayablesByContact"
        ));
    }

    let mut spec = report_params(entity, as_of);
    spec.extra
        .push(("contactId".to_owned(), contact_id.to_string()));
    spec.period_key = format!("{}__contact-{contact_id}", spec.period_key);
    Ok(spec)
}

pub fn report_params_with_options(
    entity: &EntityType,
    as_of: NaiveDate,
    options: &ReportOptions,
) -> Result<ReportSpec, String> {
    options.validate(std::slice::from_ref(entity))?;
    let base = report_params(entity, as_of);
    let mut spec = if let Some((from, to)) = options.from.zip(options.to) {
        let from = from.to_string();
        let to = to.to_string();
        ReportSpec {
            extra: vec![
                ("fromDate".into(), from.clone()),
                ("toDate".into(), to.clone()),
            ],
            period_key: format!("{from}_{to}"),
            report_date: None,
            report_from: Some(from),
            report_to: Some(to),
        }
    } else {
        base
    };
    if let Some(periods) = options.periods {
        spec.extra.push(("periods".into(), periods.to_string()));
    }
    Ok(spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 6, 17).unwrap()
    }

    #[test]
    fn report_params_asof_for_balance_sheet() {
        let spec = report_params(&EntityType::ReportBalanceSheet, run_date());
        assert_eq!(spec.period_key, "asof-2026-06-17");
        assert_eq!(spec.report_date.as_deref(), Some("2026-06-17"));
        assert!(spec.report_from.is_none());
        assert_eq!(
            spec.extra,
            vec![("date".to_owned(), "2026-06-17".to_owned())]
        );
    }

    #[test]
    fn report_params_period_for_profit_and_loss_is_month_to_date() {
        let spec = report_params(&EntityType::ReportProfitAndLoss, run_date());
        assert_eq!(spec.report_from.as_deref(), Some("2026-06-01"));
        assert_eq!(spec.report_to.as_deref(), Some("2026-06-17"));
        assert!(spec.report_date.is_none());
        assert_eq!(spec.period_key, "2026-06-01_2026-06-17");
        assert_eq!(
            spec.extra,
            vec![
                ("fromDate".to_owned(), "2026-06-01".to_owned()),
                ("toDate".to_owned(), "2026-06-17".to_owned()),
            ]
        );
    }

    #[test]
    fn report_params_signature_is_sorted() {
        let spec = report_params(&EntityType::ReportProfitAndLoss, run_date());
        // fromDate sorts before toDate.
        assert_eq!(
            spec.params_signature(),
            "fromDate=2026-06-01&toDate=2026-06-17"
        );
    }

    #[test]
    fn aged_contact_report_spec_is_contact_scoped_and_has_a_unique_raw_period_key() {
        let contact_id = uuid::Uuid::parse_str("5f3c9a2e-0000-0000-0000-000000000000").unwrap();
        let spec = aged_contact_report_params(
            &EntityType::ReportAgedReceivablesByContact,
            run_date(),
            contact_id,
        )
        .unwrap();

        assert_eq!(spec.report_date.as_deref(), Some("2026-06-17"));
        assert_eq!(
            spec.params_signature(),
            "contactId=5f3c9a2e-0000-0000-0000-000000000000&date=2026-06-17"
        );
        assert!(spec.period_key.contains("contact-5f3c9a2e"));
        assert!(aged_contact_report_params(
            &EntityType::ReportBalanceSheet,
            run_date(),
            contact_id
        )
        .is_err());
    }

    #[test]
    fn explicit_profit_and_loss_period_and_comparisons_are_preserved() {
        let options = ReportOptions {
            from: Some(NaiveDate::from_ymd_opt(2025, 1, 1).unwrap()),
            to: Some(NaiveDate::from_ymd_opt(2025, 12, 31).unwrap()),
            periods: Some(3),
        };
        let entity = EntityType::ReportProfitAndLoss;
        options.validate(std::slice::from_ref(&entity)).unwrap();
        let spec = report_params_with_options(&entity, run_date(), &options).unwrap();
        assert_eq!(spec.period_key, "2025-01-01_2025-12-31");
        assert_eq!(spec.report_from.as_deref(), Some("2025-01-01"));
        assert_eq!(spec.report_to.as_deref(), Some("2025-12-31"));
        assert_eq!(
            spec.params_signature(),
            "fromDate=2025-01-01&periods=3&toDate=2025-12-31"
        );
    }

    #[test]
    fn report_overrides_reject_unsupported_or_incomplete_requests() {
        let from = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        let to = NaiveDate::from_ymd_opt(2025, 12, 31).unwrap();
        assert!(ReportOptions {
            from: Some(from),
            to: None,
            periods: None,
        }
        .validate(&[EntityType::ReportProfitAndLoss])
        .is_err());
        assert!(ReportOptions {
            from: Some(to),
            to: Some(from),
            periods: None,
        }
        .validate(&[EntityType::ReportProfitAndLoss])
        .is_err());
        assert!(ReportOptions {
            from: None,
            to: None,
            periods: Some(13),
        }
        .validate(&[EntityType::ReportBalanceSheet])
        .is_err());
        assert!(ReportOptions {
            from: None,
            to: None,
            periods: Some(3),
        }
        .validate(&[EntityType::ReportTrialBalance])
        .is_err());
    }
}
