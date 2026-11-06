#[kernel_api_macros::envcall(class = 0)]
mod declaration {
    #[operation(slot = 0, nonreturning, reply = 3)]
    pub struct A;
}
fn main() {}
