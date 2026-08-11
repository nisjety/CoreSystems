mod normalize;
mod remote;

pub(crate) use remote::fetch_remote_recommendation;

pub(crate) fn plan_id(value: &str) -> &'static str {
    normalize::plan_id(value)
}
