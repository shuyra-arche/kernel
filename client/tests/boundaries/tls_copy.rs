fn require_copy<T: Copy>() {}
fn main() {
    require_copy::<kernel_client::__private::Tls>();
}
