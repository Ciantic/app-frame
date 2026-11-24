// `total` never declared `like`, so the method does not resolve.
#[path = "support.rs"]
mod support;
use support::Order;

fn main() {
    let c = Order::columns();

    let _ = Order::query().filters([c.total.like("5%")]);
}
