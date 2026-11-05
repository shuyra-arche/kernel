fn require_sync<T: Sync>() {}
fn main() {
    require_sync::<kernel_client::__private::Tls>();
}
