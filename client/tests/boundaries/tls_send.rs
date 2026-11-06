fn require_send<T: Send>() {}
fn main() {
    require_send::<kernel_client::__private::Tls>();
}
