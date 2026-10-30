use super::*;

pub(super) async fn run_finance(
    ctx: &mut RunContext<'_>,
    options: &ExtendedOptions,
    outcomes: &mut Vec<EntityOutcome>,
) {
    let to = options.to.expect("validated finance to date");
    for request in finance_requests(options) {
        ctx.record(request, None, Some(to), outcomes).await;
    }
}

fn finance_requests(options: &ExtendedOptions) -> Vec<ExtendedRequest> {
    let from = options.from.expect("validated finance from date");
    let to = options.to.expect("validated finance to date");
    let family = ExtendedFamily::Finance;
    let from_s = from.to_string();
    let to_s = to.to_string();
    let period = format!("{from_s}_{to_s}");
    let mut requests = Vec::new();
    for (endpoint, path, shape, params, key) in [
        (
            "finance_cash_validation",
            "/CashValidation",
            ResponseShape::Array,
            vec![("balanceDate", to_s.clone())],
            to_s.clone(),
        ),
        (
            "finance_balance_sheet",
            "/FinancialStatements/BalanceSheet",
            ResponseShape::Object,
            vec![("balanceDate", to_s.clone())],
            to_s.clone(),
        ),
        (
            "finance_cashflow",
            "/FinancialStatements/Cashflow",
            ResponseShape::Object,
            vec![("startDate", from_s.clone()), ("endDate", to_s.clone())],
            period.clone(),
        ),
        (
            "finance_profit_and_loss",
            "/FinancialStatements/ProfitAndLoss",
            ResponseShape::Object,
            vec![("startDate", from_s.clone()), ("endDate", to_s.clone())],
            period.clone(),
        ),
        (
            "finance_trial_balance",
            "/FinancialStatements/TrialBalance",
            ResponseShape::Object,
            vec![("endDate", to_s.clone())],
            to_s.clone(),
        ),
        (
            "finance_revenue_by_contact",
            "/FinancialStatements/contacts/revenue",
            ResponseShape::Object,
            vec![("startDate", from_s.clone()), ("endDate", to_s.clone())],
            period.clone(),
        ),
        (
            "finance_expense_by_contact",
            "/FinancialStatements/contacts/expense",
            ResponseShape::Object,
            vec![("startDate", from_s.clone()), ("endDate", to_s.clone())],
            period.clone(),
        ),
    ] {
        let mut req = request(family, endpoint, path, shape, false);
        req.query = params.into_iter().map(|(k, v)| (k.into(), v)).collect();
        req.request_key = Some(key);
        requests.push(req);
    }
    for bank_id in &options.bank_account_ids {
        let mut req = request(
            family,
            "finance_bank_statements_plus",
            "/BankStatementsPlus/statements",
            ResponseShape::Object,
            false,
        );
        req.query = vec![
            ("BankAccountID".into(), bank_id.to_string()),
            ("FromDate".into(), from_s.clone()),
            ("ToDate".into(), to_s.clone()),
        ];
        req.request_key = Some(format!("{bank_id}_{period}"));
        requests.push(req);
    }
    requests
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finance_requests_preserve_the_window_and_bank_account_scope() {
        let from = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 1, 31).unwrap();
        let account = Uuid::new_v4();
        let requests = finance_requests(&ExtendedOptions {
            from: Some(from),
            to: Some(to),
            bank_account_ids: vec![account],
            details: false,
        });

        assert_eq!(requests.len(), 8);
        let cashflow = requests
            .iter()
            .find(|request| request.endpoint == "finance_cashflow")
            .unwrap();
        assert_eq!(
            cashflow.request_key.as_deref(),
            Some("2026-01-01_2026-01-31")
        );
        assert!(cashflow
            .query
            .contains(&("startDate".into(), "2026-01-01".into())));
        let statements = requests
            .iter()
            .find(|request| request.endpoint == "finance_bank_statements_plus")
            .unwrap();
        let statement_key = format!("{account}_2026-01-01_2026-01-31");
        assert!(statements
            .query
            .contains(&("BankAccountID".into(), account.to_string())));
        assert_eq!(
            statements.request_key.as_deref(),
            Some(statement_key.as_str())
        );
    }
}
