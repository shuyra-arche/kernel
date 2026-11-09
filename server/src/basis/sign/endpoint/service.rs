//! A binding is an authorized service dependency, separate from event state.
//! Rebinding creates a new Arc identity; an old wait cannot inherit the new one.
use super::*;
use crate::basis::pend::{self, PendEntry};
use kernel_api::model::sign::SignRights;
pub(crate) struct Binding {
    server: Weak<Task>,
    grant: Weak<PendEntry>,
    authority: Weak<PendEntry>,
}
impl Binding {
    pub(crate) fn server(&self) -> Result<Arc<Task>, SignFail> {
        let server = self.server.upgrade().ok_or(SignFail::Dead)?;
        if !pend::live(&server) {
            return Err(SignFail::Dead);
        }
        let grant = self.grant.upgrade().ok_or(SignFail::Denied)?;
        let authority = self.authority.upgrade().ok_or(SignFail::Denied)?;
        if !server.realtime()
            || !grant.permits(kernel_api::model::pend::Operations::Sign(
                SignRights::TRIGGER,
            ))
            || !authority.permits(kernel_api::model::pend::Operations::Sign(SignRights::ROUTE))
        {
            return Err(SignFail::Denied);
        }
        Ok(server)
    }
}
pub(crate) fn assign(
    actor: &Arc<Task>,
    endpoint: &Arc<Endpoint>,
    token: PendId,
    server: &Arc<Task>,
    grant: PendId,
) -> Result<(), SignFail> {
    let _commit = crate::basis::commit();
    if !crate::basis::unit::control::manages(actor, server)
        || !endpoint.direct()
        || !server.realtime()
    {
        return Err(SignFail::Denied);
    }
    let selected = super::authorized(server, grant, SignRights::TRIGGER)?;
    if !Arc::ptr_eq(&selected, endpoint) {
        return Err(SignFail::Denied);
    }
    let authority = pend::locate(actor, token).ok_or(SignFail::Denied)?;
    let grant = pend::locate(server, grant).ok_or(SignFail::Denied)?;
    let binding = Arc::try_new(Binding {
        server: Arc::downgrade(server),
        grant: Arc::downgrade(&grant),
        authority: Arc::downgrade(&authority),
    })
    .map_err(|_| SignFail::OutOfMemory)?;
    let mut current = endpoint.service.lock();
    if current.is_some() {
        return Err(SignFail::Busy);
    }
    *current = Some(binding);
    Ok(())
}
pub(crate) fn resign(endpoint: &Endpoint) {
    endpoint.service.lock().take();
    super::authority_changed(endpoint);
}
pub(crate) fn binding(endpoint: &Endpoint) -> Result<Arc<Binding>, SignFail> {
    endpoint.service.lock().clone().ok_or(SignFail::Denied)
}
pub(crate) fn valid(endpoint: &Endpoint, binding: &Arc<Binding>) -> Result<Arc<Task>, SignFail> {
    if !endpoint
        .service
        .lock()
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(current, binding))
    {
        return Err(SignFail::Denied);
    }
    binding.server()
}
