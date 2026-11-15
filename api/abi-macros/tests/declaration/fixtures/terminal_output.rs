#[kernel_api_macros::envcall(class = 0)]
mod declaration {
    #[operation(slot=0,output=(),nonreturning)]
    pub struct A;
}
fn main() {}
