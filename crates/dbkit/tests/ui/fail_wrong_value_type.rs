// The argument type is the field type, so an integer column takes an integer.
#[path = "support.rs"]
mod support;
use support::Order;

fn main() {
    let c = Order::columns();

    let _ = Order::query().filters([c.total.gte("500")]);
}
