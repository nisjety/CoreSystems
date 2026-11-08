//! The contract of the `/v1/models` list price (EP-13-S1): an optional sibling
//! field on a chat model's entry, present only when the catalogue names a price,
//! and one switch, read by name, that is off by default.

use std::collections::BTreeSet;

use model_gateway::catalogue_prices::{ModelPrice, CATALOGUE_PRICES_ENV};

#[test]
fn price_is_a_sibling_field_or_absent() {
    let price = ModelPrice {
        input_per_million: 3.0,
        output_per_million: 15.0,
        currency: "USD".to_owned(),
        source: "prefix",
    };
    let json = serde_json::to_value(&price).expect("a price serializes");
    let keys: BTreeSet<&str> = json
        .as_object()
        .expect("a price is an object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        BTreeSet::from([
            "currency",
            "input_per_million",
            "output_per_million",
            "source"
        ]),
        "the four fields, and nothing else"
    );

    // The descriptor holds the price beside its own fields, and leaves it out when there is none,
    // so a model without a price serves the JSON it served before.
    let routes = include_str!("../src/http_routes.rs").replace("\r\n", "\n");
    let start = routes
        .find("struct ModelDescriptor {")
        .expect("the model descriptor");
    let tail = &routes[start..];
    let body = &tail[..tail.find("\n}\n").expect("the descriptor's end")];
    assert!(
        body.contains(
            "#[serde(skip_serializing_if = \"Option::is_none\")]\n    price: Option<crate::catalogue_prices::ModelPrice>,"
        ),
        "`price` is an optional field of the model entry"
    );
    assert!(
        body.contains("features: Vec<String>,"),
        "the price sits beside the features"
    );
}

#[test]
fn the_listing_prices_from_one_snapshot_behind_the_named_switch() {
    let routes = include_str!("../src/http_routes.rs").replace("\r\n", "\n");
    assert!(
        routes.contains("crate::catalogue_prices::snapshot_for_listing(&state.pricing).await"),
        "the listing takes its catalogue snapshot through the switch, once"
    );
    assert_eq!(CATALOGUE_PRICES_ENV, "MODEL_GATEWAY_CATALOGUE_PRICES");
}
