// An array column declares `contains`, not `eq`: `any` has no two-dimensional parameter, so `eq` has no operator.
#[path = "support.rs"]
mod support;
use support::Order;

fn main() {
    let c = Order::columns();

    let _ = Order::query().filters([c.tags.eq("rust")]);
}
