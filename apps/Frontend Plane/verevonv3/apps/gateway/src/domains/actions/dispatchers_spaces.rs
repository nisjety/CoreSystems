// Space action dispatchers, one module per workstream (WS03 here). The TS contract
// for each id sits in src/shared/actions/domains/spaces/<ws>.ts; the arm that routes
// the id lives in handlers.rs. Keep the list sorted.
pub(super) mod ep08;
pub(super) mod ws01;
pub(super) mod ws03;
pub(super) mod ws04;
pub(super) mod ws08;
pub(super) mod ws09;
pub(super) mod ws10;
pub(super) mod ws10b;
pub(super) mod ws11;
pub(super) mod ws12;
pub(super) mod ws14;
pub(super) mod ws16;
