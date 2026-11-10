use super::*;

pub(in crate::entry::call::cell) fn create(
    license: kernel_api::model::identity::PendId,
    at: KVirt,
    ident: &Arc<TaskIdent>,
) -> Result<(usize, usize, usize), CellFail> {
    let spec = CellCreate::from_bytes(&read_snapshot::<{ CellCreate::WIDTH }>(
        &ident.team.nest,
        at,
    )?)
    .ok_or(CellFail::NotAligned)?;
    match spec {
        CellCreate::Slot(allocation) => Ok((
            0,
            resolve_source(SourceSpec::Allocation(allocation), ident, Some(license))?,
            0,
        )),
        CellCreate::Memo {
            source,
            destination,
        } => {
            let target = destination.target();
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
                        None,
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
            let result = match destination {
                MemoDestination::Internal {
                    team,
                    placement,
                    access,
                } => create_memo(range, team, placement, access, ident),
                MemoDestination::External {
                    authority: token,
                    address,
                    access,
                } => (|| {
                    if range.offset != 0 {
                        return Err(CellFail::Denied);
                    }
                    let actor = caller(ident).ok_or(CellFail::Denied)?;
                    let _serial = actor.pend.lock();
                    let _commit = crate::basis::commit();
                    let hold = actor
                        .pend
                        .references
                        .get(range.reference.get())
                        .ok_or(CellFail::Denied)?;
                    if hold
                        .resolve()
                        .is_none_or(|r| r.page_count().get() != range.count)
                    {
                        return Err(CellFail::Denied);
                    }
                    Ok((
                        external::create(
                            &actor,
                            token,
                            hold,
                            address.get(),
                            access == kernel_api::model::modifiable::Modifiable::Yes,
                        )?,
                        address.get(),
                    ))
                })(),
            };
            let result = result.and_then(|(id, address)| {
                let actor = caller(ident).ok_or(CellFail::Denied)?;
                let mapping = MappingId::from_bytes(&(id as u64).to_le_bytes()).unwrap();
                let nest = match target {
                    MappingTarget::Cpu(team) => resolve_team(team, ident, &actor)?.nest.clone(),
                    MappingTarget::Io(token) => pend::locate(&actor, token)
                        .and_then(|p| p.target().nest())
                        .ok_or(CellFail::Denied)?,
                };
                let token = match crate::basis::nest::authority::memo::Memo::register(
                    &actor,
                    nest,
                    mapping,
                    PageCount::new(range.count).ok_or(CellFail::NotAligned)?,
                ) {
                    Ok(token) => token,
                    Err(error) => {
                        match target {
                            MappingTarget::Cpu(team) => {
                                let _ = super::delete_mapping(team, mapping, ident);
                            }
                            MappingTarget::Io(token) => {
                                let _ = external::delete(&actor, token, mapping);
                            }
                        }
                        return Err(error);
                    }
                };
                if let Some(reference) = temporary {
                    let grain = actor
                        .pend
                        .references
                        .get(reference.get())
                        .and_then(|h| h.resolve())
                        .ok_or(CellFail::Denied)?
                        .grain();
                    for page in (0..range.count).step_by(grain) {
                        if let Err(error) = associate(
                            CellAssociation {
                                source: CellPosition {
                                    object: CellObject::Slot(reference),
                                    page,
                                    generation: 0,
                                },
                                target: CellPosition {
                                    object: CellObject::Memo(token),
                                    page,
                                    generation: 1,
                                },
                            },
                            ident,
                        ) {
                            let _ = pend::return_(&actor, token);
                            return Err(error);
                        }
                    }
                }
                Ok((token.get(), address))
            });
            if let Some(r) = temporary {
                release(r, ident)?;
            }
            result.map(|(id, address)| (1, id, address))
        }
    }
}
