use super::*;
use crate::basis::cell::page::Page;
use crate::basis::unit::space::external;
use kernel_api::model::cell::*;
fn reference(id: usize) -> SlotId {
    SlotId::from_bytes(&(id as u64).to_le_bytes()).unwrap()
}
fn position(at: KVirt, ident: &Arc<TaskIdent>) -> Result<CellPosition, CellFail> {
    CellPosition::from_bytes(&read_snapshot::<{ CellPosition::WIDTH }>(
        &ident.team.space,
        at,
    )?)
    .ok_or(CellFail::NotAligned)
}
fn index(entry: &MappingEntry, page: usize) -> Result<MappingRange, CellFail> {
    entry
        .mapping()
        .ok_or(CellFail::Denied)?
        .range(PageOffset::new(page), PageCount::new(1).unwrap())
        .map_err(CellFail::from)
}
fn check(entry: &MappingEntry, actor: &Arc<Task>) -> Result<(), CellFail> {
    if !pend::live(actor) {
        return Err(CellFail::Denied);
    }
    let source = &entry.source;
    if let Some((meta, root, _)) = source.root() {
        let token = entry
            .control
            .as_ref()
            .and_then(|c| c.control().1)
            .unwrap_or(root);
        let authority = meta.authority(token).ok_or(CellFail::Denied)?;
        if !source.branch_check(Some(&authority), Access::READ) {
            return Err(CellFail::Denied);
        }
    } else if !source.private_owner(actor.ident.id) {
        return Err(CellFail::Denied);
    }
    Ok(())
}
pub(super) fn create(at: KVirt, ident: &Arc<TaskIdent>) -> Result<(usize, usize, usize), CellFail> {
    let spec = CellCreate::from_bytes(&read_snapshot::<{ CellCreate::WIDTH }>(
        &ident.team.space,
        at,
    )?)
    .ok_or(CellFail::NotAligned)?;
    match spec {
        CellCreate::Slot(allocation) => Ok((
            0,
            resolve_source(SourceSpec::Allocation(allocation), ident)?,
            0,
        )),
        CellCreate::Memo {
            target,
            source,
            placement,
            access,
            control,
        } => {
            let mut temporary = None;
            let range = match source {
                CellSource::Slot(range) => range,
                CellSource::Pend {
                    token,
                    offset,
                    count,
                } => {
                    let actor = caller(ident).ok_or(CellFail::Denied)?;
                    let parent = pend::locate(&actor, token).ok_or(CellFail::Denied)?;
                    let base = parent
                        .authority()
                        .window()
                        .map_or(0, |(offset, _)| offset.get());
                    let offset = base.checked_add(offset).ok_or(CellFail::NotAligned)?;
                    let id = resolve_source(
                        SourceSpec::Source {
                            token,
                            offset,
                            count,
                        },
                        ident,
                    )?;
                    let r = reference(id);
                    temporary = Some(r);
                    ReferenceRange {
                        reference: r,
                        offset: 0,
                        count,
                    }
                }
            };
            let result = match target {
                MappingTarget::Cpu(team) => {
                    create_memo(range, team, placement, access, control, ident)
                }
                MappingTarget::Io(token) => (|| {
                    if control != MappingControl::Caller
                        || !matches!(access, 2 | 6)
                        || range.offset != 0
                        || range.count != 1
                    {
                        return Err(CellFail::Denied);
                    }
                    let kernel_api::model::cell::PlacementAt::Exact(address) = placement.at()
                    else {
                        return Err(CellFail::Denied);
                    };
                    let actor = caller(ident).ok_or(CellFail::Denied)?;
                    let _serial = actor.pend.lock();
                    let _commit = crate::basis::commit();
                    let hold = actor
                        .pend
                        .references
                        .get(range.reference.get())
                        .ok_or(CellFail::Denied)?;
                    Ok((
                        external::create(&actor, token, hold, address.get(), access == 6)?,
                        address.get(),
                    ))
                })(),
            };
            let result = if let (Some(reference), Ok((id, address))) = (temporary, result) {
                let mapping = MappingId::from_bytes(&(id as u64).to_le_bytes()).unwrap();
                let object = CellObject::Memo { target, mapping };
                let result = (|| {
                    // A Pend supplies an authorized association, never a new owning Slot.
                    for page in 0..range.count {
                        handoff(
                            CellHandoff {
                                source: CellPosition {
                                    object: CellObject::Slot(reference),
                                    page,
                                    generation: 0,
                                },
                                target: CellPosition {
                                    object,
                                    page,
                                    generation: 1,
                                },
                            },
                            ident,
                        )?;
                    }
                    Ok((id, address))
                })();
                if result.is_err() {
                    match target {
                        MappingTarget::Cpu(team) => {
                            let _ = super::delete_mapping(team, mapping, ident);
                        }
                        MappingTarget::Io(token) => {
                            if let Some(actor) = caller(ident) {
                                let _ = external::delete(&actor, token, mapping);
                            }
                        }
                    }
                }
                result
            } else {
                result
            };
            if let Some(r) = temporary {
                release(r, ident)?;
            }
            result.map(|(id, address)| (1, id, address))
        }
    }
}
/// Returns a Page snapshot and the exact authority range that permits association.
fn source(pos: CellPosition, ident: &Arc<TaskIdent>) -> Result<(FlatRange, Arc<Page>), CellFail> {
    let actor = caller(ident).ok_or(CellFail::Denied)?;
    match pos.object {
        CellObject::Slot(r) => {
            let _serial = actor.pend.lock();
            let hold = actor.pend.references.get(r.get()).ok_or(CellFail::Denied)?;
            let range = hold
                .resolve()
                .ok_or(CellFail::Denied)?
                .range(PageOffset::new(pos.page), PageCount::new(1).unwrap())?;
            // Fresh Slot capacity acquires its zeroed Page on first use; Missing never regenerates.
            let cell = crate::basis::cell::Cell::Flat(range.clone());
            let page = match cell.page(0) {
                Ok(page) => page,
                Err(NestError::Missing) => {
                    let _materialized = range.materialize()?;
                    cell.page(0)?
                }
                Err(error) => return Err(error.into()),
            };
            Ok((range, page))
        }
        CellObject::Memo {
            target: MappingTarget::Cpu(team),
            mapping,
        } => with_mapping(team, mapping, ident, |target, entry| {
            check(entry, &actor)?;
            let range = index(entry, pos.page)?;
            let info = target.space.nest.inspect_memo(range.mapping(), pos.page)?;
            if info.generation != pos.generation {
                return Err(CellFail::Busy);
            }
            Ok((
                range.source()?,
                crate::basis::cell::Cell::Nest(range).page(0)?,
            ))
        }),
        CellObject::Memo {
            target: MappingTarget::Io(token),
            mapping,
        } => {
            if pos.page != 0 {
                return Err(CellFail::NoRegion);
            }
            external::page(&actor, token, mapping, pos.generation)
        }
    }
}
fn inspect(pos: CellPosition, ident: &Arc<TaskIdent>) -> Result<CellInfo, CellFail> {
    let actor = caller(ident).ok_or(CellFail::Denied)?;
    match pos.object {
        CellObject::Slot(r) => {
            let _serial = actor.pend.lock();
            let range = endpoint(&actor, r)?;
            let range = range.range(PageOffset::new(pos.page), PageCount::new(1).unwrap())?;
            let page = range
                .materialized_cells()?
                .into_iter()
                .next()
                .map(|(_, p)| p);
            Ok(CellInfo {
                generation: 0,
                state: if page.is_some() {
                    CellState::Ready
                } else {
                    CellState::Vacant
                },
                page: page.as_ref().map_or(0, |p| p.id().get() as u64),
                frozen: page.as_ref().is_some_and(|p| p.frozen()),
                external: page
                    .as_ref()
                    .is_some_and(|p| p.exposure() == crate::basis::cell::page::Exposure::External),
                address: KVirt::new(0),
                fault_cause: 0,
                fault_address: 0,
            })
        }
        CellObject::Memo {
            target: MappingTarget::Cpu(team),
            mapping,
        } => with_mapping(team, mapping, ident, |target, entry| {
            let range = index(entry, pos.page)?;
            // Observation remains available while a remote hart has yet to fence.
            let _ = target.space.nest.retry_invalidation();
            target.space.nest.finish_memo(range.mapping(), pos.page);
            Ok(target.space.nest.inspect_memo(range.mapping(), pos.page)?)
        }),
        CellObject::Memo {
            target: MappingTarget::Io(token),
            mapping,
        } => {
            if pos.page != 0 {
                return Err(CellFail::NoRegion);
            }
            external::snapshot(&actor, token, mapping)
        }
    }
}
fn handoff(spec: CellHandoff, ident: &Arc<TaskIdent>) -> Result<usize, CellFail> {
    if matches!(spec.target.object, CellObject::Memo { .. })
        && let CellObject::Memo {
            target: MappingTarget::Cpu(team),
            mapping,
        } = spec.source.object
    {
        let actor = caller(ident).ok_or(CellFail::Denied)?;
        return with_mapping(team, mapping, ident, |target, entry| {
            check(entry, &actor)?;
            let range = index(entry, spec.source.page)?;
            if target
                .space
                .nest
                .inspect_memo(range.mapping(), spec.source.page)?
                .generation
                != spec.source.generation
            {
                return Err(CellFail::Busy);
            }
            let source = range.source()?;
            let page = crate::basis::cell::Cell::Nest(range).page(0)?;
            handoff_prepared(spec, ident, Some((source, page)))
        });
    }
    handoff_prepared(spec, ident, None)
}
fn handoff_prepared(
    spec: CellHandoff,
    ident: &Arc<TaskIdent>,
    mut prepared: Option<(FlatRange, Arc<Page>)>,
) -> Result<usize, CellFail> {
    let actor = caller(ident).ok_or(CellFail::Denied)?;
    match spec.target.object {
        CellObject::Slot(to) => match spec.source.object {
            CellObject::Slot(from) => {
                transfer(from, spec.source.page, to, spec.target.page, None, ident)
            }
            CellObject::Memo {
                target: MappingTarget::Cpu(team),
                mapping,
            } => {
                let id = resolve_source(
                    SourceSpec::Mapping {
                        team,
                        mapping,
                        offset: spec.source.page,
                        count: 1,
                    },
                    ident,
                )?;
                let r = reference(id);
                let result = with_mapping(team, mapping, ident, |target, entry| {
                    if !entry.source.manages(&actor) {
                        return Err(CellFail::NotTransferable);
                    }
                    check(entry, &actor)?;
                    let selected = index(entry, spec.source.page)?;
                    if target
                        .space
                        .nest
                        .inspect_memo(selected.mapping(), spec.source.page)?
                        .generation
                        != spec.source.generation
                    {
                        return Err(CellFail::Busy);
                    }
                    let page = crate::basis::cell::Cell::Nest(selected).page(0)?;
                    transfer(r, 0, to, spec.target.page, Some(&page), ident)
                });
                release(r, ident)?;
                result
            }
            CellObject::Memo {
                target: MappingTarget::Io(token),
                mapping,
            } => {
                if spec.source.page != 0 {
                    return Err(CellFail::NoRegion);
                }
                let (r, page) = {
                    let _serial = actor.pend.lock();
                    let (hold, page) =
                        external::owning_hold(&actor, token, mapping, spec.source.generation)?;
                    let (offset, count) = hold.window();
                    let id = actor
                        .pend
                        .references
                        .insert(
                            hold.source().clone(),
                            offset,
                            count,
                            hold.ceiling(),
                            hold.branch()
                                .map(|(token, authority)| (token, authority.clone())),
                        )
                        .map_err(source_fail)?;
                    (reference(id), page)
                };
                let result = transfer(r, 0, to, spec.target.page, Some(&page), ident);
                release(r, ident)?;
                result
            }
        },
        CellObject::Memo {
            target: MappingTarget::Cpu(team),
            mapping,
        } => {
            let (from, page) = match prepared.take() {
                Some(source) => source,
                None => source(spec.source, ident)?,
            };
            with_mapping(team, mapping, ident, |target, entry| {
                check(entry, &actor)?;
                let selected = index(entry, spec.target.page)?;
                if !selected.source()?.same_range(&from) {
                    return Err(CellFail::Denied);
                }
                let root = entry.source.root();
                let _operation = root
                    .as_ref()
                    .map(|(meta, _, _)| ShareOperation::acquire(meta).ok_or(CellFail::Busy))
                    .transpose()?;
                check(entry, &actor)?;
                target.space.nest.associate(
                    selected.mapping(),
                    spec.target.page,
                    spec.target.generation,
                    page,
                )?;
                Ok(0)
            })
        }
        CellObject::Memo {
            target: MappingTarget::Io(token),
            mapping,
        } => {
            if spec.target.page != 0 {
                return Err(CellFail::NoRegion);
            }
            let (from, page) = match prepared.take() {
                Some(source) => source,
                None => source(spec.source, ident)?,
            };
            let _serial = actor.pend.lock();
            let _commit = crate::basis::commit();
            external::associate(&actor, token, mapping, spec.target.generation, &from, page)?;
            Ok(0)
        }
    }
}
fn publication(
    pos: CellPosition,
    publishing: bool,
    ident: &Arc<TaskIdent>,
) -> Result<usize, CellFail> {
    let actor = caller(ident).ok_or(CellFail::Denied)?;
    match pos.object {
        CellObject::Memo {
            target: MappingTarget::Cpu(team),
            mapping,
        } => with_mapping(team, mapping, ident, |target, entry| {
            check(entry, &actor)?;
            let selected = index(entry, pos.page)?;
            target.space.nest.retry_invalidation()?;
            target.space.nest.finish_memo(selected.mapping(), pos.page);
            let info = target
                .space
                .nest
                .inspect_memo(selected.mapping(), pos.page)?;
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
                .map(|(meta, _, _)| ShareOperation::acquire(meta).ok_or(CellFail::Busy))
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
            let flight = next_flight_key()?;
            if publishing {
                entry
                    .source
                    .enter_publish(flight, authority.as_deref(), need)
                    .map_err(source_fail)?;
            }
            let result = (|| {
                target.space.nest.transition_memo(
                    selected.mapping(),
                    pos.page,
                    pos.generation,
                    permit.clone(),
                    publishing,
                )?;
                if publishing {
                    target.space.nest.push_page(
                        selected.mapping(),
                        KVirt::new(selected.window().0),
                        &page,
                        &permit,
                    )
                } else {
                    target.space.nest.unpublish_page(
                        selected.mapping(),
                        KVirt::new(selected.window().0),
                        &page,
                    )
                }
            })();
            let valid = entry.source.branch_check(authority.as_deref(), need) && pend::live(&actor);
            if publishing && !valid {
                let _ = target.space.nest.unpublish_page(
                    selected.mapping(),
                    KVirt::new(selected.window().0),
                    &page,
                );
            }
            if publishing {
                entry.source.leave_publish(flight);
            }
            target.space.nest.finish_memo(selected.mapping(), pos.page);
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
        }),
        CellObject::Memo {
            target: MappingTarget::Io(token),
            mapping,
        } => {
            if pos.page != 0 {
                return Err(CellFail::NoRegion);
            }
            let _serial = actor.pend.lock();
            let _commit = crate::basis::commit();
            if publishing {
                external::publish(&actor, token, mapping, pos.generation)?;
            } else {
                external::retract(&actor, token, mapping, pos.generation)?;
            }
            Ok(0)
        }
        CellObject::Slot(_) => Err(CellFail::Denied),
    }
}
pub(super) fn execute(call: CellCall, ident: &Arc<TaskIdent>) -> Result<usize, CellFail> {
    match call {
        CellCall::Create { .. } => unreachable!(),
        CellCall::Push { request } | CellCall::Pull { request } => handoff(
            CellHandoff::from_bytes(&read_snapshot::<{ CellHandoff::WIDTH }>(
                &ident.team.space,
                request,
            )?)
            .ok_or(CellFail::NotAligned)?,
            ident,
        ),
        CellCall::Publish { position: at } => publication(position(at, ident)?, true, ident),
        CellCall::Retract { position: at } => publication(position(at, ident)?, false, ident),
        CellCall::Inspect { position: at, out } => {
            let info = inspect(position(at, ident)?, ident)?;
            if !ident.team.space.copy_out(&info.to_bytes(), out) {
                return Err(CellFail::Denied);
            }
            Ok(0)
        }
        CellCall::Protect {
            position: at,
            access,
        } => {
            let pos = position(at, ident)?;
            let CellObject::Memo {
                target: MappingTarget::Cpu(team),
                mapping,
            } = pos.object
            else {
                return Err(CellFail::Denied);
            };
            let access = Access::from_wire(access)
                .filter(|a| a.is_leaf_set())
                .ok_or(CellFail::Denied)?;
            let actor = caller(ident).ok_or(CellFail::Denied)?;
            with_mapping(team, mapping, ident, |target, entry| {
                check(entry, &actor)?;
                target.space.nest.retry_invalidation()?;
                let selected = index(entry, pos.page)?;
                let mapping = selected.mapping();
                target.space.nest.finish_memo(mapping, pos.page);
                {
                    let mut inner = mapping.state.lock();
                    let memo = inner.memo_mut(pos.page)?;
                    if memo.generation != pos.generation || memo.flight.is_some() {
                        return Err(CellFail::Busy);
                    }
                    memo.generation = memo.generation.checked_add(1).ok_or(CellFail::OoM)?;
                }
                target.space.access(selected, access)?;
                if access.contains(Access::EXECUTE) {
                    identity::sync_instructions()?;
                }
                Ok(0)
            })
        }
        CellCall::Delete { object: at } => {
            let object = CellObject::from_bytes(&read_snapshot::<{ CellObject::WIDTH }>(
                &ident.team.space,
                at,
            )?)
            .ok_or(CellFail::NotAligned)?;
            match object {
                CellObject::Slot(r) => release(r, ident),
                CellObject::Memo {
                    target: MappingTarget::Cpu(team),
                    mapping,
                } => with_mapping(team, mapping, ident, |target, entry| {
                    target.space.nest.retry_invalidation()?;
                    let relation = entry.mapping().ok_or(CellFail::Denied)?;
                    if !relation.state.lock().published.is_empty() {
                        return Err(CellFail::Busy);
                    }
                    target.space.withdraw(relation.whole())?;
                    if target.space.nest.settled(mapping.get()) {
                        target.space.retire_mapping(mapping.get());
                    }
                    Ok(0)
                }),
                CellObject::Memo {
                    target: MappingTarget::Io(token),
                    mapping,
                } => {
                    let actor = caller(ident).ok_or(CellFail::Denied)?;
                    let _serial = actor.pend.lock();
                    let _commit = crate::basis::commit();
                    external::delete(&actor, token, mapping)?;
                    Ok(0)
                }
            }
        }
    }
}
