#[kernel_api_macros::envcall(class = 2)]
mod declaration {
    #[operation(slot=0,output=(),error=Missing)]
    pub struct Release;
}
fn main() {}
