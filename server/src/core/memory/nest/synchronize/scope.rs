/// 本批 PTE 修改需要的 CPU 失效范围；失败重试不能缩小范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    AddressSpace,
    Global,
}

impl Scope {
    pub(crate) fn include(self, global: bool) -> Self {
        if global { Self::Global } else { self }
    }
}
