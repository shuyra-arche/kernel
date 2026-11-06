/// Whether all cleanup finished or remains owned by the existing TLS table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cleanup {
    Complete,
    Deferred,
}
