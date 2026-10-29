//! Explicit, operator-approved Xero writes. No MCP caller can approve its own request.

use crate::{backend::Failure, config::AccessScope};
use anyhow::Result;
use chrono::{Duration as ChronoDuration, NaiveDate, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Number, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use uuid::Uuid;

const APPROVAL_SECONDS: i64 = 900;
const ACCOUNTING_BASE_URL: &str = "https://api.xero.com/api.xro/2.0";
const PAYROLL_NZ_BASE_URL: &str = "https://api.xero.com/payroll.xro/2.0";

/// The full mutation surface published by Xero's current MCP server.
///
/// Actions are stored and approved by their Xero operation name, not by the
/// generic compatibility input that carries them. This keeps the existing
/// per-client action grants meaningful.
pub const OFFICIAL_MCP_ACTIONS: &[&str] = &[
    "create_contact",
    "create_invoice",
    "create_payment",
    "create_bank_transaction",
    "create_credit_note",
    "create_item",
    "create_manual_journal",
    "create_quote",
    "create_payroll_timesheet",
    "create_tracking_category",
    "create_tracking_options",
    "update_contact",
    "update_invoice",
    "update_bank_transaction",
    "update_credit_note",
    "update_item",
    "update_manual_journal",
    "update_quote",
    "update_tracking_category",
    "update_tracking_options",
    "approve_payroll_timesheet",
    "add_payroll_timesheet_line",
    "update_payroll_timesheet_line",
    "revert_payroll_timesheet",
    "delete_payroll_timesheet",
];

pub const ACTIONS: &[&str] = &[
    "create_contact",
    "update_contact",
    "create_draft_invoice",
    "create_invoice",
    "create_payment",
    "create_bank_transaction",
    "create_credit_note",
    "create_item",
    "create_manual_journal",
    "create_quote",
    "create_payroll_timesheet",
    "create_tracking_category",
    "create_tracking_options",
    "update_invoice",
    "update_bank_transaction",
    "update_credit_note",
    "update_item",
    "update_manual_journal",
    "update_quote",
    "update_tracking_category",
    "update_tracking_options",
    "approve_payroll_timesheet",
    "add_payroll_timesheet_line",
    "update_payroll_timesheet_line",
    "revert_payroll_timesheet",
    "delete_payroll_timesheet",
];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WriteRequest {
    pub tenant_id: String,
    #[serde(flatten)]
    pub action: WriteAction,
}

#[derive(Clone, Debug)]
pub struct PreparedWrite {
    pub action: String,
    pub method: reqwest::Method,
    pub base_url: String,
    pub path: String,
    pub body: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "action", content = "input", rename_all = "snake_case")]
pub enum WriteAction {
    CreateContact(ContactInput),
    UpdateContact(UpdateContactInput),
    CreateDraftInvoice(InvoiceInput),
    CreatePayment(PaymentInput),
    /// Provides the remaining official Xero MCP mutations using a fixed,
    /// audited route registry. It never accepts a caller-defined HTTP method,
    /// host, or path template.
    OfficialMcp(OfficialMcpWriteInput),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OfficialMcpWriteInput {
    pub operation: String,
    #[serde(default)]
    pub path: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Option<Value>,
}

#[derive(Clone, Copy, Debug)]
pub struct OfficialMcpWriteSpec {
    pub operation: &'static str,
    pub method: &'static str,
    pub base_url: &'static str,
    pub path_template: &'static str,
    pub path_parameters: &'static [&'static str],
    pub body_required: bool,
    pub response_collection: Option<(&'static str, &'static str)>,
}

const OFFICIAL_MCP_WRITE_SPECS: &[OfficialMcpWriteSpec] = &[
    OfficialMcpWriteSpec {
        operation: "create_contact",
        method: "PUT",
        base_url: ACCOUNTING_BASE_URL,
        path_template: "Contacts",
        path_parameters: &[],
        body_required: true,
        response_collection: Some(("Contacts", "ContactID")),
    },
    OfficialMcpWriteSpec {
        operation: "create_invoice",
        method: "PUT",
        base_url: ACCOUNTING_BASE_URL,
        path_template: "Invoices",
        path_parameters: &[],
        body_required: true,
        response_collection: Some(("Invoices", "InvoiceID")),
    },
    OfficialMcpWriteSpec {
        operation: "create_payment",
        method: "POST",
        base_url: ACCOUNTING_BASE_URL,
        path_template: "Payments",
        path_parameters: &[],
        body_required: true,
        response_collection: Some(("Payments", "PaymentID")),
    },
    OfficialMcpWriteSpec {
        operation: "create_bank_transaction",
        method: "PUT",
        base_url: ACCOUNTING_BASE_URL,
        path_template: "BankTransactions",
        path_parameters: &[],
        body_required: true,
        response_collection: Some(("BankTransactions", "BankTransactionID")),
    },
    OfficialMcpWriteSpec {
        operation: "create_credit_note",
        method: "PUT",
        base_url: ACCOUNTING_BASE_URL,
        path_template: "CreditNotes",
        path_parameters: &[],
        body_required: true,
        response_collection: Some(("CreditNotes", "CreditNoteID")),
    },
    OfficialMcpWriteSpec {
        operation: "create_item",
        method: "PUT",
        base_url: ACCOUNTING_BASE_URL,
        path_template: "Items",
        path_parameters: &[],
        body_required: true,
        response_collection: Some(("Items", "ItemID")),
    },
    OfficialMcpWriteSpec {
        operation: "create_manual_journal",
        method: "PUT",
        base_url: ACCOUNTING_BASE_URL,
        path_template: "ManualJournals",
        path_parameters: &[],
        body_required: true,
        response_collection: Some(("ManualJournals", "ManualJournalID")),
    },
    OfficialMcpWriteSpec {
        operation: "create_quote",
        method: "PUT",
        base_url: ACCOUNTING_BASE_URL,
        path_template: "Quotes",
        path_parameters: &[],
        body_required: true,
        response_collection: Some(("Quotes", "QuoteID")),
    },
    OfficialMcpWriteSpec {
        operation: "create_payroll_timesheet",
        method: "POST",
        base_url: PAYROLL_NZ_BASE_URL,
        path_template: "Timesheets",
        path_parameters: &[],
        body_required: true,
        response_collection: Some(("Timesheet", "TimesheetID")),
    },
    OfficialMcpWriteSpec {
        operation: "create_tracking_category",
        method: "PUT",
        base_url: ACCOUNTING_BASE_URL,
        path_template: "TrackingCategories",
        path_parameters: &[],
        body_required: true,
        response_collection: Some(("TrackingCategories", "TrackingCategoryID")),
    },
    OfficialMcpWriteSpec {
        operation: "create_tracking_options",
        method: "PUT",
        base_url: ACCOUNTING_BASE_URL,
        path_template: "TrackingCategories/{TrackingCategoryID}/Options",
        path_parameters: &["TrackingCategoryID"],
        body_required: true,
        response_collection: Some(("Options", "TrackingOptionID")),
    },
    OfficialMcpWriteSpec {
        operation: "update_contact",
        method: "POST",
        base_url: ACCOUNTING_BASE_URL,
        path_template: "Contacts/{ContactID}",
        path_parameters: &["ContactID"],
        body_required: true,
        response_collection: Some(("Contacts", "ContactID")),
    },
    OfficialMcpWriteSpec {
        operation: "update_invoice",
        method: "POST",
        base_url: ACCOUNTING_BASE_URL,
        path_template: "Invoices/{InvoiceID}",
        path_parameters: &["InvoiceID"],
        body_required: true,
        response_collection: Some(("Invoices", "InvoiceID")),
    },
    OfficialMcpWriteSpec {
        operation: "update_bank_transaction",
        method: "POST",
        base_url: ACCOUNTING_BASE_URL,
        path_template: "BankTransactions/{BankTransactionID}",
        path_parameters: &["BankTransactionID"],
        body_required: true,
        response_collection: Some(("BankTransactions", "BankTransactionID")),
    },
    OfficialMcpWriteSpec {
        operation: "update_credit_note",
        method: "POST",
        base_url: ACCOUNTING_BASE_URL,
        path_template: "CreditNotes/{CreditNoteID}",
        path_parameters: &["CreditNoteID"],
        body_required: true,
        response_collection: Some(("CreditNotes", "CreditNoteID")),
    },
    OfficialMcpWriteSpec {
        operation: "update_item",
        method: "POST",
        base_url: ACCOUNTING_BASE_URL,
        path_template: "Items/{ItemID}",
        path_parameters: &["ItemID"],
        body_required: true,
        response_collection: Some(("Items", "ItemID")),
    },
    OfficialMcpWriteSpec {
        operation: "update_manual_journal",
        method: "POST",
        base_url: ACCOUNTING_BASE_URL,
        path_template: "ManualJournals/{ManualJournalID}",
        path_parameters: &["ManualJournalID"],
        body_required: true,
        response_collection: Some(("ManualJournals", "ManualJournalID")),
    },
    OfficialMcpWriteSpec {
        operation: "update_quote",
        method: "POST",
        base_url: ACCOUNTING_BASE_URL,
        path_template: "Quotes/{QuoteID}",
        path_parameters: &["QuoteID"],
        body_required: true,
        response_collection: Some(("Quotes", "QuoteID")),
    },
    OfficialMcpWriteSpec {
        operation: "update_tracking_category",
        method: "POST",
        base_url: ACCOUNTING_BASE_URL,
        path_template: "TrackingCategories/{TrackingCategoryID}",
        path_parameters: &["TrackingCategoryID"],
        body_required: true,
        response_collection: Some(("TrackingCategories", "TrackingCategoryID")),
    },
    OfficialMcpWriteSpec {
        operation: "update_tracking_options",
        method: "POST",
        base_url: ACCOUNTING_BASE_URL,
        path_template: "TrackingCategories/{TrackingCategoryID}/Options/{TrackingOptionID}",
        path_parameters: &["TrackingCategoryID", "TrackingOptionID"],
        body_required: true,
        response_collection: Some(("Options", "TrackingOptionID")),
    },
    OfficialMcpWriteSpec {
        operation: "approve_payroll_timesheet",
        method: "POST",
        base_url: PAYROLL_NZ_BASE_URL,
        path_template: "Timesheets/{TimesheetID}/Approve",
        path_parameters: &["TimesheetID"],
        body_required: false,
        response_collection: Some(("Timesheet", "TimesheetID")),
    },
    OfficialMcpWriteSpec {
        operation: "add_payroll_timesheet_line",
        method: "POST",
        base_url: PAYROLL_NZ_BASE_URL,
        path_template: "Timesheets/{TimesheetID}/Lines",
        path_parameters: &["TimesheetID"],
        body_required: true,
        response_collection: Some(("TimesheetLine", "TimesheetLineID")),
    },
    OfficialMcpWriteSpec {
        operation: "update_payroll_timesheet_line",
        method: "PUT",
        base_url: PAYROLL_NZ_BASE_URL,
        path_template: "Timesheets/{TimesheetID}/Lines/{TimesheetLineID}",
        path_parameters: &["TimesheetID", "TimesheetLineID"],
        body_required: true,
        response_collection: Some(("TimesheetLine", "TimesheetLineID")),
    },
    OfficialMcpWriteSpec {
        operation: "revert_payroll_timesheet",
        method: "POST",
        base_url: PAYROLL_NZ_BASE_URL,
        path_template: "Timesheets/{TimesheetID}/RevertToDraft",
        path_parameters: &["TimesheetID"],
        body_required: false,
        response_collection: Some(("Timesheet", "TimesheetID")),
    },
    OfficialMcpWriteSpec {
        operation: "delete_payroll_timesheet",
        method: "DELETE",
        base_url: PAYROLL_NZ_BASE_URL,
        path_template: "Timesheets/{TimesheetID}",
        path_parameters: &["TimesheetID"],
        body_required: false,
        response_collection: None,
    },
];

pub fn official_write_actions() -> Vec<&'static str> {
    OFFICIAL_MCP_ACTIONS.to_vec()
}

pub fn official_mcp_write_spec(operation: &str) -> Option<&'static OfficialMcpWriteSpec> {
    OFFICIAL_MCP_WRITE_SPECS
        .iter()
        .find(|spec| spec.operation == operation)
}

pub fn write_base_url(action: &str) -> Option<&'static str> {
    match action {
        "create_draft_invoice" => Some(ACCOUNTING_BASE_URL),
        _ => official_mcp_write_spec(action).map(|spec| spec.base_url),
    }
}

pub fn write_response_collection(action: &str) -> Option<(&'static str, &'static str)> {
    match action {
        "create_draft_invoice" => Some(("Invoices", "InvoiceID")),
        _ => official_mcp_write_spec(action).and_then(|spec| spec.response_collection),
    }
}

pub fn official_mcp_write_operations() -> Vec<Value> {
    OFFICIAL_MCP_WRITE_SPECS
        .iter()
        .map(|spec| {
            json!({
                "operation": spec.operation,
                "method": spec.method,
                "api_base": spec.base_url,
                "path_template": spec.path_template,
                "path_parameters": spec.path_parameters,
                "body_required": spec.body_required,
                "approval_required": true,
            })
        })
        .collect()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContactInput {
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateContactInput {
    pub contact_id: String,
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InvoiceInput {
    pub contact_id: String,
    pub invoice_type: InvoiceType,
    pub date: Option<String>,
    pub due_date: Option<String>,
    pub reference: Option<String>,
    pub line_items: Vec<InvoiceLine>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub enum InvoiceType {
    ACCREC,
    ACCPAY,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InvoiceLine {
    pub description: String,
    pub quantity: Number,
    pub unit_amount: Number,
    pub account_code: String,
    pub tax_type: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PaymentInput {
    pub invoice_id: String,
    pub account_id: String,
    pub amount: Number,
    pub date: Option<String>,
    pub reference: Option<String>,
}

fn valid_text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

fn optional_text(value: &Option<String>, max: usize) -> bool {
    value.as_ref().is_none_or(|value| valid_text(value, max))
}

fn valid_date(value: &Option<String>) -> bool {
    value
        .as_ref()
        .is_none_or(|date| NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok())
}

fn valid_email(value: &Option<String>) -> bool {
    value.as_ref().is_none_or(|email| {
        valid_text(email, 255)
            && !email.chars().any(char::is_whitespace)
            && email
                .split('@')
                .collect::<Vec<_>>()
                .as_slice()
                .iter()
                .all(|part| !part.is_empty())
            && email.matches('@').count() == 1
    })
}

fn valid_decimal(value: &Number, positive: bool) -> bool {
    let raw = value.to_string();
    let mut parts = raw.split('.');
    let whole = parts.next().unwrap_or("");
    let fraction = parts.next();
    let numeric = value.as_f64().unwrap_or(f64::NAN);
    (if positive {
        numeric > 0.0
    } else {
        numeric >= 0.0
    }) && numeric.is_finite()
        && whole.len() <= 12
        && whole.bytes().all(|c| c.is_ascii_digit())
        && fraction
            .is_none_or(|f| !f.is_empty() && f.len() <= 4 && f.bytes().all(|c| c.is_ascii_digit()))
        && parts.next().is_none()
}

impl WriteRequest {
    pub fn validate(&self) -> Result<(), Failure> {
        Uuid::parse_str(&self.tenant_id)
            .map_err(|_| Failure::invalid("tenant_id must be a UUID."))?;
        self.action.validate()
    }

    pub fn prepare(&self) -> Result<PreparedWrite, Failure> {
        self.validate()?;
        let (method, path) = self.action.method_path()?;
        Ok(PreparedWrite {
            action: self.action.name().to_owned(),
            method,
            base_url: self.action.base_url()?.to_owned(),
            path,
            body: self.action.body(),
        })
    }
}

impl WriteAction {
    pub fn name(&self) -> &str {
        match self {
            Self::CreateContact(_) => "create_contact",
            Self::UpdateContact(_) => "update_contact",
            Self::CreateDraftInvoice(_) => "create_draft_invoice",
            Self::CreatePayment(_) => "create_payment",
            Self::OfficialMcp(input) => &input.operation,
        }
    }

    pub fn method_path(&self) -> Result<(reqwest::Method, String), Failure> {
        let target = match self {
            Self::CreateContact(_) => (reqwest::Method::PUT, "Contacts".into()),
            Self::UpdateContact(input) => (
                reqwest::Method::POST,
                format!("Contacts/{}", input.contact_id),
            ),
            Self::CreateDraftInvoice(_) => (reqwest::Method::PUT, "Invoices".into()),
            Self::CreatePayment(_) => (reqwest::Method::POST, "Payments".into()),
            Self::OfficialMcp(input) => {
                let spec = official_mcp_write_spec(&input.operation).ok_or_else(|| {
                    Failure::invalid("Unsupported official Xero MCP write operation.")
                })?;
                let mut path = spec.path_template.to_owned();
                for parameter in spec.path_parameters {
                    let value = input.path.get(*parameter).ok_or_else(|| {
                        Failure::invalid("Missing required path identifier for Xero write.")
                    })?;
                    path = path.replace(&format!("{{{parameter}}}"), value);
                }
                (
                    reqwest::Method::from_bytes(spec.method.as_bytes())
                        .map_err(|_| Failure::invalid("Invalid allowlisted Xero write method."))?,
                    path,
                )
            }
        };
        Ok(target)
    }

    pub fn base_url(&self) -> Result<&'static str, Failure> {
        match self {
            Self::OfficialMcp(input) => official_mcp_write_spec(&input.operation)
                .map(|spec| spec.base_url)
                .ok_or_else(|| Failure::invalid("Unsupported official Xero MCP write operation.")),
            _ => Ok(ACCOUNTING_BASE_URL),
        }
    }

    pub fn validate(&self) -> Result<(), Failure> {
        let valid = match self {
            Self::CreateContact(input) => {
                valid_text(&input.name, 255)
                    && valid_email(&input.email)
                    && optional_text(&input.phone, 50)
            }
            Self::UpdateContact(input) => {
                Uuid::parse_str(&input.contact_id).is_ok()
                    && valid_text(&input.name, 255)
                    && valid_email(&input.email)
                    && optional_text(&input.phone, 50)
            }
            Self::CreateDraftInvoice(input) => {
                Uuid::parse_str(&input.contact_id).is_ok()
                    && valid_date(&input.date)
                    && valid_date(&input.due_date)
                    && match (&input.date, &input.due_date) {
                        (Some(date), Some(due)) => due >= date,
                        _ => true,
                    }
                    && optional_text(&input.reference, 255)
                    && (1..=20).contains(&input.line_items.len())
                    && input.line_items.iter().all(|line| {
                        valid_text(&line.description, 500)
                            && valid_text(&line.account_code, 50)
                            && optional_text(&line.tax_type, 50)
                            && valid_decimal(&line.quantity, true)
                            && valid_decimal(&line.unit_amount, false)
                    })
            }
            Self::CreatePayment(input) => {
                Uuid::parse_str(&input.invoice_id).is_ok()
                    && Uuid::parse_str(&input.account_id).is_ok()
                    && valid_decimal(&input.amount, true)
                    && valid_date(&input.date)
                    && optional_text(&input.reference, 255)
            }
            Self::OfficialMcp(input) => validate_official_mcp_input(input),
        };
        if valid {
            Ok(())
        } else {
            Err(Failure::invalid(
                "Invalid Xero write input; check identifiers, dates, amounts and field lengths.",
            ))
        }
    }

    pub fn body(&self) -> Value {
        let today = || Utc::now().date_naive().format("%Y-%m-%d").to_string();
        match self {
            Self::CreateContact(input) => {
                let mut contact = json!({"Name":input.name});
                add_contact_fields(&mut contact, &input.email, &input.phone);
                json!({"Contacts":[contact]})
            }
            Self::UpdateContact(input) => {
                let mut contact = json!({"ContactID":input.contact_id,"Name":input.name});
                add_contact_fields(&mut contact, &input.email, &input.phone);
                json!({"Contacts":[contact]})
            }
            Self::CreateDraftInvoice(input) => {
                let date = input.date.clone().unwrap_or_else(today);
                let due = input.due_date.clone().unwrap_or_else(|| {
                    NaiveDate::parse_from_str(&date, "%Y-%m-%d")
                        .expect("validated date")
                        .checked_add_signed(ChronoDuration::days(30))
                        .unwrap_or_else(|| Utc::now().date_naive())
                        .format("%Y-%m-%d")
                        .to_string()
                });
                let lines = input.line_items.iter().map(|line| {
                    let mut item = json!({"Description":line.description,"Quantity":line.quantity,"UnitAmount":line.unit_amount,"AccountCode":line.account_code});
                    if let Some(tax) = &line.tax_type { item["TaxType"] = json!(tax); }
                    item
                }).collect::<Vec<_>>();
                let mut invoice = json!({"Type":input.invoice_type,"Contact":{"ContactID":input.contact_id},"Date":date,"DueDate":due,"Status":"DRAFT","LineItems":lines});
                if let Some(reference) = &input.reference {
                    match input.invoice_type {
                        InvoiceType::ACCREC => invoice["Reference"] = json!(reference),
                        InvoiceType::ACCPAY => invoice["InvoiceNumber"] = json!(reference),
                    }
                }
                json!({"Invoices":[invoice]})
            }
            Self::CreatePayment(input) => {
                let mut payment = json!({"Invoice":{"InvoiceID":input.invoice_id},"Account":{"AccountID":input.account_id},"Amount":input.amount,"Date":input.date.clone().unwrap_or_else(today)});
                if let Some(reference) = &input.reference {
                    payment["Reference"] = json!(reference);
                }
                payment
            }
            Self::OfficialMcp(input) => input.body.clone().unwrap_or(Value::Null),
        }
    }
}

fn validate_official_mcp_input(input: &OfficialMcpWriteInput) -> bool {
    let Some(spec) = official_mcp_write_spec(&input.operation) else {
        return false;
    };
    if input.path.len() != spec.path_parameters.len()
        || !spec.path_parameters.iter().all(|name| {
            input
                .path
                .get(*name)
                .is_some_and(|value| Uuid::parse_str(value).is_ok())
        })
    {
        return false;
    }
    match (&input.body, spec.body_required) {
        (None, false) => true,
        (Some(body), _) => valid_write_body(body),
        (None, true) => false,
    }
}

fn valid_write_body(value: &Value) -> bool {
    const MAX_DEPTH: usize = 12;
    const MAX_NODES: usize = 1_000;
    const MAX_BYTES: usize = 64 * 1024;
    if !value.is_object() || serde_json::to_vec(value).map_or(true, |body| body.len() > MAX_BYTES) {
        return false;
    }
    fn visit(value: &Value, depth: usize, nodes: &mut usize) -> bool {
        if depth > MAX_DEPTH || *nodes >= MAX_NODES {
            return false;
        }
        *nodes += 1;
        match value {
            Value::Null | Value::Bool(_) | Value::Number(_) => true,
            Value::String(text) => text.len() <= 4_096 && !text.chars().any(char::is_control),
            Value::Array(items) => {
                items.len() <= 100 && items.iter().all(|item| visit(item, depth + 1, nodes))
            }
            Value::Object(fields) => {
                fields.len() <= 100
                    && fields.iter().all(|(key, value)| {
                        !key.is_empty()
                            && key.len() <= 128
                            && key.chars().all(|character| {
                                character.is_ascii_alphanumeric() || character == '_'
                            })
                            && visit(value, depth + 1, nodes)
                    })
            }
        }
    }
    let mut nodes = 0;
    visit(value, 0, &mut nodes)
}

fn add_contact_fields(contact: &mut Value, email: &Option<String>, phone: &Option<String>) {
    if let Some(email) = email {
        contact["EmailAddress"] = json!(email);
    }
    if let Some(phone) = phone {
        contact["Phones"] = json!([{"PhoneType":"MOBILE","PhoneNumber":phone}]);
    }
}

pub fn tool_schema() -> Value {
    let id = json!({"type":"string","format":"uuid"});
    let text = json!({"type":"string","minLength":1});
    let date = json!({"type":"string","format":"date"});
    let money = json!({"type":"number","exclusiveMinimum":0});
    let form = |action: &str, input: Value, required: Vec<&str>| json!({"type":"object","properties":{"tenant_id":id,"action":{"const":action},"input":{"type":"object","properties":input,"required":required,"additionalProperties":false}},"required":["tenant_id","action","input"],"additionalProperties":false});
    let official_operation = json!({
        "type":"object",
        "properties":{
            "operation":{"type":"string","enum":OFFICIAL_MCP_ACTIONS},
            "path":{"type":"object","additionalProperties":{"type":"string","format":"uuid"}},
            "body":{"type":"object","additionalProperties":true}
        },
        "required":["operation"],
        "additionalProperties":false
    });
    json!({"type":"object","oneOf":[
        form("create_contact",json!({"name":text,"email":{"type":"string","format":"email"},"phone":text}),vec!["name"]),
        form("update_contact",json!({"contact_id":id,"name":text,"email":{"type":"string","format":"email"},"phone":text}),vec!["contact_id","name"]),
        form("create_draft_invoice",json!({"contact_id":id,"invoice_type":{"type":"string","enum":["ACCREC","ACCPAY"]},"date":date,"due_date":date,"reference":text,"line_items":{"type":"array","minItems":1,"maxItems":20,"items":{"type":"object","properties":{"description":text,"quantity":money,"unit_amount":{"type":"number","minimum":0},"account_code":text,"tax_type":text},"required":["description","quantity","unit_amount","account_code"],"additionalProperties":false}}}),vec!["contact_id","invoice_type","line_items"]),
        form("create_payment",json!({"invoice_id":id,"account_id":id,"amount":money,"date":date,"reference":text}),vec!["invoice_id","account_id","amount"]),
        form("official_mcp",official_operation,vec!["operation"])
    ]})
}

#[derive(Clone, Debug)]
pub struct WriteCommand {
    pub id: Uuid,
    pub tenant_id: String,
    pub action: String,
    pub method: reqwest::Method,
    pub path: String,
    pub body: Value,
    pub idempotency_key: String,
}

#[derive(Clone, Copy)]
pub enum WriteOutcome {
    Committed,
    Rejected,
    NotSent,
    Uncertain,
}

impl WriteOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Committed => "committed",
            Self::Rejected => "rejected",
            Self::NotSent => "not_sent",
            Self::Uncertain => "outcome_uncertain",
        }
    }
}

struct StoredWriteRow {
    client_id: String,
    tenant_id: String,
    action: String,
    method: String,
    path: String,
    body: String,
    key: String,
    expires_at: i64,
    approved_at: Option<i64>,
    status: String,
}

#[derive(Clone)]
pub struct WriteStore(Arc<Mutex<Connection>>);

impl WriteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            if !path.exists() {
                let _ = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(path)?;
            }
            let metadata = std::fs::symlink_metadata(path)?;
            anyhow::ensure!(
                metadata.file_type().is_file() && metadata.permissions().mode() & 0o077 == 0,
                "MCP write database must be a private regular file"
            );
        }
        let connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(3))?;
        connection.pragma_update(None, "secure_delete", "ON")?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS writes (id TEXT PRIMARY KEY, client_id TEXT NOT NULL, tenant_id TEXT NOT NULL, action TEXT NOT NULL, method TEXT NOT NULL, path TEXT NOT NULL, body TEXT NOT NULL, idempotency_key TEXT NOT NULL, created_at INTEGER NOT NULL, expires_at INTEGER NOT NULL, approved_at INTEGER, claimed_at INTEGER, completed_at INTEGER, status TEXT NOT NULL DEFAULT 'prepared'); CREATE INDEX IF NOT EXISTS writes_expiry ON writes(expires_at);")?;
        Ok(Self(Arc::new(Mutex::new(connection))))
    }

    fn connection(&self) -> Result<std::sync::MutexGuard<'_, Connection>, Failure> {
        self.0.lock().map_err(|_| {
            Failure::new(
                "write_store_unavailable",
                "Write approval storage is unavailable.",
                true,
            )
        })
    }

    pub fn inspect(&self, id: Uuid) -> Result<Value> {
        let connection = self
            .connection()
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let row: Option<(String, String, String, String, i64, String)> = connection.query_row(
            "SELECT client_id, tenant_id, action, body, expires_at, status FROM writes WHERE id = ?1",
            [id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
        ).optional()?;
        let (client_id, tenant_id, action, body, expires_at, status) =
            row.ok_or_else(|| anyhow::anyhow!("write approval was not found"))?;
        let body_sha256 = format!("{:x}", Sha256::digest(body.as_bytes()));
        Ok(
            json!({"approval_id":id,"client_id":client_id,"tenant_id":tenant_id,"action":action,"body":serde_json::from_str::<Value>(&body)?,"body_sha256":body_sha256,"expires_at":expires_at,"status":status}),
        )
    }

    pub fn prepare(&self, scope: &AccessScope, request: WriteRequest) -> Result<Value, Failure> {
        request.validate()?;
        scope.authorize_write(&request.tenant_id, request.action.name())?;
        let request = WriteRequest {
            tenant_id: Uuid::parse_str(&request.tenant_id)
                .expect("validated tenant")
                .to_string(),
            action: request.action,
        };
        let id = Uuid::new_v4();
        let key = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let (method, path) = request.action.method_path()?;
        let body = request.action.body();
        let action = request.action.name().to_owned();
        let expires_at = Utc::now().timestamp() + APPROVAL_SECONDS;
        let connection = self.connection()?;
        connection.execute("INSERT INTO writes (id, client_id, tenant_id, action, method, path, body, idempotency_key, created_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)", params![id.to_string(), scope.client_id, request.tenant_id, action, method.as_str(), path, body.to_string(), key, Utc::now().timestamp(), expires_at]).map_err(store_error)?;
        Ok(
            json!({"approval_id":id,"tenant_id":request.tenant_id,"action":request.action.name(),"preview":body,"expires_at":expires_at,"approval":"A server operator must review and approve this exact request out of band."}),
        )
    }

    pub fn approve(
        &self,
        id: Uuid,
        client: &str,
        tenant: &str,
        action: &str,
        body_sha256: &str,
    ) -> Result<()> {
        Uuid::parse_str(tenant)?;
        anyhow::ensure!(ACTIONS.contains(&action), "unknown write action");
        anyhow::ensure!(
            body_sha256.len() == 64 && body_sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "invalid body digest"
        );
        let mut connection = self
            .connection()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let body: Option<String> = tx.query_row("SELECT body FROM writes WHERE id = ?1 AND client_id = ?2 AND tenant_id = ?3 AND action = ?4 AND status = 'prepared' AND approved_at IS NULL AND expires_at > ?5", params![id.to_string(), client, tenant, action, Utc::now().timestamp()], |row| row.get(0)).optional()?;
        let body = body.ok_or_else(|| {
            anyhow::anyhow!("matching pending write was not found or has expired")
        })?;
        anyhow::ensure!(
            format!("{:x}", Sha256::digest(body.as_bytes())) == body_sha256.to_ascii_lowercase(),
            "body digest does not match the inspected request"
        );
        let updated = tx.execute(
            "UPDATE writes SET approved_at = ?2 WHERE id = ?1",
            params![id.to_string(), Utc::now().timestamp()],
        )?;
        anyhow::ensure!(
            updated == 1,
            "matching pending write was not found or has expired"
        );
        tx.commit()?;
        Ok(())
    }

    pub fn claim(&self, scope: &AccessScope, id: Uuid) -> Result<WriteCommand, Failure> {
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_error)?;
        let row: Option<StoredWriteRow> = tx.query_row("SELECT client_id,tenant_id,action,method,path,body,idempotency_key,expires_at,approved_at,status FROM writes WHERE id = ?1", [id.to_string()], |row| Ok(StoredWriteRow { client_id: row.get(0)?, tenant_id: row.get(1)?, action: row.get(2)?, method: row.get(3)?, path: row.get(4)?, body: row.get(5)?, key: row.get(6)?, expires_at: row.get(7)?, approved_at: row.get(8)?, status: row.get(9)? })).optional().map_err(store_error)?;
        let Some(row) = row else {
            return Err(Failure::new(
                "write_not_found",
                "Write approval was not found.",
                false,
            ));
        };
        if row.client_id != scope.client_id {
            return Err(Failure::new(
                "write_forbidden",
                "This write belongs to another MCP client.",
                false,
            ));
        }
        scope.authorize_write(&row.tenant_id, &row.action)?;
        if row.status != "prepared" {
            return Err(Failure::new("write_already_claimed", "This write has already been submitted or resolved; inspect Xero before preparing another.", false));
        }
        if row.expires_at <= Utc::now().timestamp() {
            return Err(Failure::new(
                "write_expired",
                "This write approval has expired.",
                false,
            ));
        }
        if row.approved_at.is_none() {
            return Err(Failure::new(
                "write_not_approved",
                "A server operator must approve this write before submission.",
                false,
            ));
        }
        let method = reqwest::Method::from_bytes(row.method.as_bytes()).map_err(store_error)?;
        let body = serde_json::from_str(&row.body).map_err(store_error)?;
        tx.execute(
            "UPDATE writes SET status = 'claimed', claimed_at = ?2 WHERE id = ?1",
            params![id.to_string(), Utc::now().timestamp()],
        )
        .map_err(store_error)?;
        tx.commit().map_err(store_error)?;
        Ok(WriteCommand {
            id,
            tenant_id: row.tenant_id,
            action: row.action,
            method,
            path: row.path,
            body,
            idempotency_key: row.key,
        })
    }

    pub fn finish(&self, id: Uuid, outcome: WriteOutcome) -> Result<(), Failure> {
        let connection = self.connection()?;
        let updated = connection.execute("UPDATE writes SET status = ?2, completed_at = ?3 WHERE id = ?1 AND status = 'claimed'", params![id.to_string(), outcome.as_str(), Utc::now().timestamp()]).map_err(store_error)?;
        if updated != 1 {
            return Err(store_error("missing claimed write"));
        }
        Ok(())
    }
}

fn store_error(_: impl std::fmt::Display) -> Failure {
    Failure::new(
        "write_store_unavailable",
        "Write approval storage is unavailable.",
        true,
    )
}
