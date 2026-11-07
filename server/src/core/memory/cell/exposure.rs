/// Cell 的外部访问风险；Internal 不表示普通持有或 CPU 访问已经结束。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Exposure {
    Internal,
    External,
}
