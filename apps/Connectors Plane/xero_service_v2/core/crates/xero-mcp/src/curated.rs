//! Ergonomic, tenant-scoped shortcuts over the pinned Xero OpenAPI catalog.
//!
//! These helpers never call an endpoint by URL. They only select a fixed,
//! catalogued operation and let the normal query path validate and execute it.

use crate::{
    backend::{Failure, Query},
    catalog::Params,
};
use serde::Deserialize;
use serde_json::{json, Value};
use url::form_urlencoded;
use uuid::Uuid;

fn default_limit() -> usize {
    50
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusinessResource {
    Accounts,
    BankTransactions,
    Budgets,
    Contacts,
    CreditNotes,
    Currencies,
    Invoices,
    Items,
    Journals,
    ManualJournals,
    Organisations,
    Payments,
    PurchaseOrders,
    Quotes,
    TaxRates,
    TrackingCategories,
}

impl BusinessResource {
    fn operation_id(self) -> &'static str {
        match self {
            Self::Accounts => "getAccounts",
            Self::BankTransactions => "getBankTransactions",
            Self::Budgets => "getBudgets",
            Self::Contacts => "getContacts",
            Self::CreditNotes => "getCreditNotes",
            Self::Currencies => "getCurrencies",
            Self::Invoices => "getInvoices",
            Self::Items => "getItems",
            Self::Journals => "getJournals",
            Self::ManualJournals => "getManualJournals",
            Self::Organisations => "getOrganisations",
            Self::Payments => "getPayments",
            Self::PurchaseOrders => "getPurchaseOrders",
            Self::Quotes => "getQuotes",
            Self::TaxRates => "getTaxRates",
            Self::TrackingCategories => "getTrackingCategories",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Accounts => "accounts",
            Self::BankTransactions => "bank_transactions",
            Self::Budgets => "budgets",
            Self::Contacts => "contacts",
            Self::CreditNotes => "credit_notes",
            Self::Currencies => "currencies",
            Self::Invoices => "invoices",
            Self::Items => "items",
            Self::Journals => "journals",
            Self::ManualJournals => "manual_journals",
            Self::Organisations => "organisations",
            Self::Payments => "payments",
            Self::PurchaseOrders => "purchase_orders",
            Self::Quotes => "quotes",
            Self::TaxRates => "tax_rates",
            Self::TrackingCategories => "tracking_categories",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BusinessReadArgs {
    pub tenant_id: String,
    pub resource: BusinessResource,
    #[serde(default)]
    pub query: Params,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

impl BusinessReadArgs {
    pub fn into_query(self) -> Query {
        Query {
            tenant_id: self.tenant_id,
            family: "accounting".into(),
            operation_id: self.resource.operation_id().into(),
            path: Params::default(),
            query: self.query,
            header: Params::default(),
            limit: self.limit,
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinancialReport {
    AgedPayables,
    AgedReceivables,
    BalanceSheet,
    ProfitAndLoss,
    TrialBalance,
}

impl FinancialReport {
    fn operation_id(self) -> &'static str {
        match self {
            Self::AgedPayables => "getReportAgedPayablesByContact",
            Self::AgedReceivables => "getReportAgedReceivablesByContact",
            Self::BalanceSheet => "getReportBalanceSheet",
            Self::ProfitAndLoss => "getReportProfitAndLoss",
            Self::TrialBalance => "getReportTrialBalance",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::AgedPayables => "aged_payables",
            Self::AgedReceivables => "aged_receivables",
            Self::BalanceSheet => "balance_sheet",
            Self::ProfitAndLoss => "profit_and_loss",
            Self::TrialBalance => "trial_balance",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinancialReportArgs {
    pub tenant_id: String,
    pub report: FinancialReport,
    #[serde(default)]
    pub query: Params,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

impl FinancialReportArgs {
    pub fn into_query(self) -> Query {
        Query {
            tenant_id: self.tenant_id,
            family: "accounting".into(),
            operation_id: self.report.operation_id().into(),
            path: Params::default(),
            query: self.query,
            header: Params::default(),
            limit: self.limit,
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PayrollNzResource {
    EarningsRates,
    Employees,
    LeaveTypes,
    PayRunCalendars,
    PayRuns,
    PaySlips,
    Timesheets,
}

impl PayrollNzResource {
    fn operation_id(self) -> &'static str {
        match self {
            Self::EarningsRates => "getEarningsRates",
            Self::Employees => "getEmployees",
            Self::LeaveTypes => "getLeaveTypes",
            Self::PayRunCalendars => "getPayRunCalendars",
            Self::PayRuns => "getPayRuns",
            Self::PaySlips => "getPaySlips",
            Self::Timesheets => "getTimesheets",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::EarningsRates => "earnings_rates",
            Self::Employees => "employees",
            Self::LeaveTypes => "leave_types",
            Self::PayRunCalendars => "pay_run_calendars",
            Self::PayRuns => "pay_runs",
            Self::PaySlips => "pay_slips",
            Self::Timesheets => "timesheets",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PayrollNzReadArgs {
    pub tenant_id: String,
    pub resource: PayrollNzResource,
    #[serde(default)]
    pub query: Params,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

impl PayrollNzReadArgs {
    pub fn into_query(self) -> Query {
        Query {
            tenant_id: self.tenant_id,
            family: "payroll_nz".into(),
            operation_id: self.resource.operation_id().into(),
            path: Params::default(),
            query: self.query,
            header: Params::default(),
            limit: self.limit,
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiResource {
    BankTransaction,
    Bill,
    Contact,
    CreditNote,
    Invoice,
    ManualJournal,
    Payment,
    Quote,
}

impl UiResource {
    pub fn requires_short_code(self) -> bool {
        !matches!(self, Self::BankTransaction | Self::ManualJournal)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::BankTransaction => "bank_transaction",
            Self::Bill => "bill",
            Self::Contact => "contact",
            Self::CreditNote => "credit_note",
            Self::Invoice => "invoice",
            Self::ManualJournal => "manual_journal",
            Self::Payment => "payment",
            Self::Quote => "quote",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiLinkArgs {
    pub tenant_id: String,
    pub resource: UiResource,
    pub resource_id: String,
    #[serde(default)]
    pub bank_account_id: Option<String>,
}

pub fn build_ui_link(
    args: UiLinkArgs,
    organization_short_code: Option<&str>,
) -> Result<Value, Failure> {
    let tenant_id = parse_uuid(&args.tenant_id, "tenant_id")?;
    let resource_id = parse_uuid(&args.resource_id, "resource_id")?;
    let short_code = if args.resource.requires_short_code() {
        Some(validate_short_code(organization_short_code)?)
    } else {
        None
    };
    let url = match args.resource {
        UiResource::Invoice => format!(
            "https://go.xero.com/app/{}/invoicing/view/{resource_id}",
            short_code.expect("invoice short code checked")
        ),
        UiResource::Bill => organization_redirect(
            short_code.expect("bill short code checked"),
            &format!("/AccountsPayable/Edit.aspx?InvoiceID={resource_id}"),
        ),
        UiResource::Contact => format!(
            "https://go.xero.com/app/{}/contacts/contact/{resource_id}",
            short_code.expect("contact short code checked")
        ),
        UiResource::CreditNote => organization_redirect(
            short_code.expect("credit note short code checked"),
            &format!("/AccountsPayable/ViewCreditNote.aspx?creditNoteID={resource_id}"),
        ),
        UiResource::Quote => format!(
            "https://go.xero.com/app/{}/quotes/view/{resource_id}",
            short_code.expect("quote short code checked")
        ),
        UiResource::Payment => organization_redirect(
            short_code.expect("payment short code checked"),
            &format!("/Bank/ViewTransaction.aspx?bankTransactionID={resource_id}"),
        ),
        UiResource::ManualJournal => {
            format!("https://go.xero.com/Journal/View.aspx?invoiceID={resource_id}")
        }
        UiResource::BankTransaction => {
            let account_id = parse_uuid(
                args.bank_account_id.as_deref().ok_or_else(|| {
                    Failure::invalid("bank_account_id is required for bank_transaction links.")
                })?,
                "bank_account_id",
            )?;
            format!(
                "https://go.xero.com/Bank/ViewTransaction.aspx?bankTransactionID={resource_id}&accountID={account_id}"
            )
        }
    };
    Ok(json!({
        "data": {
            "tenant_id": tenant_id,
            "resource": args.resource.as_str(),
            "resource_id": resource_id,
            "url": url,
        },
        "meta": {"coverage": "xero_ui_navigation"},
    }))
}

fn parse_uuid(value: &str, field: &str) -> Result<String, Failure> {
    Uuid::parse_str(value)
        .map(|value| value.hyphenated().to_string())
        .map_err(|_| Failure::invalid(&format!("{field} must be a UUID.")))
}

fn validate_short_code(value: Option<&str>) -> Result<&str, Failure> {
    let value = value.ok_or_else(|| {
        Failure::invalid("organization_short_code is required for this Xero resource link.")
    })?;
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(Failure::invalid(
            "organization_short_code must contain only ASCII letters, digits, or hyphens.",
        ));
    }
    Ok(value)
}

fn organization_redirect(short_code: &str, destination: &str) -> String {
    let destination = form_urlencoded::byte_serialize(destination.as_bytes()).collect::<String>();
    format!(
        "https://go.xero.com/organisationlogin/default.aspx?shortcode={short_code}&redirecturl={destination}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const TENANT: &str = "5f3c9a2e-0000-0000-0000-000000000000";
    const RESOURCE: &str = "8d02390c-8d05-4a5e-9e78-099d49c07b0d";

    #[test]
    fn curated_resources_map_only_to_fixed_catalog_operations() {
        let read: BusinessReadArgs = serde_json::from_value(json!({
            "tenant_id": TENANT,
            "resource": "invoices",
            "query": {"page": 2},
        }))
        .unwrap();
        let query = read.into_query();
        assert_eq!(query.family, "accounting");
        assert_eq!(query.operation_id, "getInvoices");
        assert_eq!(query.query["page"], 2);

        let payroll: PayrollNzReadArgs = serde_json::from_value(json!({
            "tenant_id": TENANT,
            "resource": "employees",
        }))
        .unwrap();
        assert_eq!(payroll.into_query().operation_id, "getEmployees");
    }

    #[test]
    fn ui_links_require_valid_identifiers_and_safe_short_codes() {
        let link = build_ui_link(
            serde_json::from_value(json!({
                "tenant_id": TENANT,
                "resource": "invoice",
                "resource_id": RESOURCE,
            }))
            .unwrap(),
            Some("ABCD1234"),
        )
        .unwrap();
        assert_eq!(
            link["data"]["url"],
            format!("https://go.xero.com/app/ABCD1234/invoicing/view/{RESOURCE}")
        );
        for short_code in ["../bad", "with space", ""] {
            let args: UiLinkArgs = serde_json::from_value(json!({
                "tenant_id": TENANT,
                "resource": "invoice",
                "resource_id": RESOURCE,
            }))
            .unwrap();
            assert!(build_ui_link(args, Some(short_code)).is_err());
        }
        let missing: UiLinkArgs = serde_json::from_value(json!({
            "tenant_id": TENANT,
            "resource": "invoice",
            "resource_id": RESOURCE,
        }))
        .unwrap();
        assert!(build_ui_link(missing, None).is_err());
    }
}
