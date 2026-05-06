pub const DEFERRED_PAYMENT_MESSAGE: &str = "payments and settlement are not implemented in this Radroots release; order coordination is available now, and payment support is planned for a future phase";

pub fn is_deferred_payment_operation(operation_id: &str) -> bool {
    matches!(
        operation_id,
        "order.payment.record" | "order.settlement.accept" | "order.settlement.reject"
    )
}

pub fn deferred_payment_message() -> String {
    DEFERRED_PAYMENT_MESSAGE.to_owned()
}
