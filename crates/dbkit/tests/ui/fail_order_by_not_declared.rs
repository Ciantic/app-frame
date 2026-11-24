// `total` did not declare `#[db(orderable)]`.
#[path = "support.rs"]
mod support;
use support::Order;

fn main() {
    let c = Order::columns();

    let _ = Order::query().order_by([c.total.desc()]);
}
