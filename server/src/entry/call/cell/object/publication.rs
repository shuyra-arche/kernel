use super::*;

pub(super) fn publication(
    pos: CellPosition,
    publishing: bool,
    ident: &Arc<TaskIdent>,
) -> Result<usize, CellFail> {
    let actor = caller(ident).ok_or(CellFail::Denied)?;
    let rights = if publishing {
        MemoRights::PUBLISH
    } else {
        MemoRights::RETRACT
    };
    with_memo(pos, rights, ident, |lease, at| {
        if lease.memo().nest.is_external() {
            if at != 0 {
                return Err(CellFail::NoRegion);
            }
            let _serial = actor.pend.lock();
            let _commit = crate::basis::commit();
            if publishing {
                external::publish(
                    &actor,
                    lease.0.token(),
                    lease.memo().mapping,
                    pos.generation,
                )?;
            } else {
                external::retract(
                    &actor,
                    lease.0.token(),
                    lease.memo().mapping,
                    pos.generation,
                )?;
            }
            return Ok(0);
        }
        cpu(lease, |nest, entry| {
            check(entry, &actor)?;
            let selected = index(entry, at)?;
            nest.retry_invalidation()?;
            nest.finish_memo(selected.mapping(), at);
            let info = selected.mapping().inspect_memo(at)?;
            if info.generation != pos.generation {
                return Err(CellFail::Busy);
            }
            if publishing && info.state != CellState::Ready {
                return Err(CellFail::AlreadyMapped);
            }
            if !publishing && info.state != CellState::Published {
                return Err(CellFail::Busy);
            }
            let page = crate::basis::cell::Cell::Nest(selected.clone()).page(0)?;
            let permit = crate::basis::cell::change::acquire(&page)?;
            let root = entry.source.root();
            let _operation = root
                .as_ref()
                .map(|(meta, _, _)| SharingOperation::acquire(meta).ok_or(CellFail::Busy))
                .transpose()?;
            let branch = entry.control.as_ref().and_then(|c| c.control().1);
            let authority = root
                .as_ref()
                .and_then(|(meta, token, _)| meta.authority(branch.unwrap_or(*token)));
            let need = selected
                .mapping()
                .state
                .lock()
                .range_at(selected.window().0)
                .ok_or(CellFail::NoRegion)?
                .ceiling;
            if publishing && need.contains(Access::WRITE) && lease.0.modifiable() != Modifiable::Yes
            {
                return Err(CellFail::Denied);
            }
            let flight = next_flight_key()?;
            if publishing {
                let base = entry
                    .source
                    .full_range()
                    .ok_or(CellFail::Denied)?
                    .index()
                    .get();
                let range = selected.source()?;
                entry
                    .source
                    .enter_publish(
                        flight,
                        authority.as_deref(),
                        need,
                        PageOffset::new(range.index().get() - base),
                        range.page_count(),
                    )
                    .map_err(source_fail)?;
            }
            let result = (|| {
                nest.transition_memo(
                    selected.mapping(),
                    at,
                    pos.generation,
                    permit.clone(),
                    publishing.then(|| lease.0.authority().clone()),
                )?;
                if publishing {
                    nest.push_page(
                        selected.mapping(),
                        KVirt::new(selected.window().0),
                        &page,
                        &permit,
                    )
                } else {
                    nest.unpublish_page(selected.mapping(), KVirt::new(selected.window().0), &page)
                }
            })();
            let valid = entry.source.branch_check(authority.as_deref(), need)
                && pend::live(&actor)
                && lease.0.alive();
            if publishing && !valid {
                let _ =
                    nest.unpublish_page(selected.mapping(), KVirt::new(selected.window().0), &page);
            }
            if publishing {
                entry.source.leave_publish(flight);
            }
            nest.finish_memo(selected.mapping(), at);
            if !valid {
                return Err(CellFail::Denied);
            }
            result?;
            if publishing && need.contains(Access::EXECUTE) {
                identity::sync_instructions()?;
            }
            if let Some((space, mapping)) = selected.mapping().wake_key() {
                sign::signal(WakeKey::Mapping { space, mapping });
            }
            Ok(0)
        })
    })
}
