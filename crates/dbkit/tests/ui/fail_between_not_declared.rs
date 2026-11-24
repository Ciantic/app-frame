// `total` declared `gte` and `lt`, not `between`.
#[path = "support.rs"]
mod support;
use support::Order;

fn main() {
    let c = Order::columns();

    let _ = Order::query().filters([c.total.between(1, 2)]);
}
