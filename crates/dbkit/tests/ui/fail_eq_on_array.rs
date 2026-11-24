// An array column declares `contains`, not `eq`: `any` has no two-dimensional
// parameter, so the plan gives it no value operator at all.
#[path = "support.rs"]
mod support;
use support::Order;

fn main() {
    let c = Order::columns();

    let _ = Order::query().filters([c.tags.eq("rust")]);
}
